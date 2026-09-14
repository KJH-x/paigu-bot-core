use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::domain::claim::Eligibility;
use crate::domain::event::EventEnvelope;
use crate::domain::ids::{RoundId, UserId};
use crate::domain::item::RoundContext;
use crate::domain::round::{Round, RoundConfig, RoundStatus};
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidationOutcome};
use crate::replay::replay_engine::{ReplayEngine, ReplayOptions};
use crate::simulation::verifier::{load_round_fixture, make_eligibility, RoundFixture};

struct RawMsg {
    message_id: String,
    user_id: String,
    nickname: String,
    text: String,
    timestamp_ms: i64,
    is_admin: bool,
}

struct ChatSession {
    fixture: RoundFixture,
    round_contexts: Vec<RoundContext>,
    validator: EventValidator,
    eligibility: Vec<Eligibility>,
    fund_users: HashSet<String>,
    events: Vec<EventEnvelope>,
    messages: Vec<RawMsg>,
    seen: HashSet<String>,
    round_closed: bool,
    seq: i64,
}

impl ChatSession {
    fn new(fixture: RoundFixture) -> Self {
        let round_contexts = vec![RoundContext {
            round_id: RoundId(fixture.round_id.clone()),
            title: fixture.title.clone(),
            items: fixture.items.clone(),
        }];
        Self {
            fixture,
            round_contexts,
            validator: EventValidator::new(0.65),
            eligibility: vec![],
            fund_users: HashSet::new(),
            events: vec![],
            messages: vec![],
            seen: HashSet::new(),
            round_closed: false,
            seq: 0,
        }
    }

    fn round_id(&self) -> RoundId {
        RoundId(self.fixture.round_id.clone())
    }

    async fn handle(&mut self, req: &ChatRequest) -> Value {
        let now = Utc::now();
        let text = req.text.trim().to_string();
        let is_admin = req.is_admin.unwrap_or(false);
        let nickname = req.nickname.clone().unwrap_or_else(|| req.user_id.clone());
        self.seq += 1;
        let message_id = format!("live_{}", self.seq);

        self.messages.push(RawMsg {
            message_id: message_id.clone(),
            user_id: req.user_id.clone(),
            nickname: nickname.clone(),
            text: text.clone(),
            timestamp_ms: now.timestamp_millis(),
            is_admin,
        });

        if text.is_empty() {
            return json!({ "reply": "", "status": "Ignored", "detail": "空消息" });
        }

        if text.starts_with('/') {
            if !is_admin {
                return json!({ "reply": "此命令仅管理员可用。", "status": "Unsupported", "detail": "非管理员斜杠命令" });
            }
            if text.starts_with("/加优先") {
                let mut nick_map = std::collections::HashMap::new();
                nick_map.insert(nickname.clone(), req.user_id.clone());
                for m in &self.messages {
                    nick_map.insert(m.nickname.clone(), m.user_id.clone());
                }
                let rid = self.round_id();
                match crate::simulation::verifier::parse_admin_eligibility(
                    &text, &nick_map, &self.fixture.items, &rid, now,
                ) {
                    Some(e) => {
                        if e.note.as_deref().unwrap_or("").contains("购物金") {
                            self.fund_users.insert(e.user_id.0.clone());
                        }
                        let msg = format!("已授权优先权 user={} level={}", e.user_id.0, e.priority_level);
                        self.eligibility.push(e);
                        return json!({ "reply": msg, "status": "Applied", "detail": msg });
                    }
                    None => {
                        return json!({ "reply": "无法解析 /加优先", "status": "Unsupported", "detail": "" });
                    }
                }
            }
            if text.starts_with("/结团") {
                self.round_closed = true;
                return json!({ "reply": "已结团", "status": "Applied", "detail": "结团" });
            }
            if text.starts_with("/重置") {
                self.events.clear();
                self.messages.clear();
                self.seen.clear();
                self.eligibility.clear();
                self.fund_users.clear();
                self.round_closed = false;
                self.seq = 0;
                return json!({ "reply": "已重置模拟", "status": "Applied", "detail": "reset" });
            }
            return json!({ "reply": "管理员命令已记录", "status": "Applied", "detail": text });
        }

        if self.round_closed {
            return json!({ "reply": "团已结团，拒绝排谷。", "status": "Rejected", "detail": "RoundClosed" });
        }

        if text.contains("购物金") && !text.contains("非购物金") && !self.fund_users.contains(&req.user_id) {
            self.fund_users.insert(req.user_id.clone());
            let rid = self.round_id();
            self.eligibility.push(make_eligibility(&rid, &req.user_id, 10, None, "购物金(自述)", now));
        }

        let parsed = RuleParser::parse(&text, &self.fixture.items, is_admin);
        if parsed.intent == ParsedIntent::Modify {
            return json!({ "reply": "改单功能暂未实现。", "status": "Unsupported", "detail": "Modify" });
        }

        let user_id = UserId(req.user_id.clone());
        let validation = self
            .validator
            .validate(
                parsed,
                &user_id,
                &self.fixture.group_id,
                Some(message_id.clone()),
                &self.round_contexts,
                now,
                self.seq,
            )
            .await;

        match validation {
            Ok(ValidationOutcome::Ok(event)) => {
                let detail = crate::engine::replay::describe_event(&event);
                self.events.push(event);
                let (version, snapshot) = self.rebuild().await;
                json!({
                    "reply": format!("已记录，当前版本 #{}", version),
                    "status": "Applied",
                    "detail": detail,
                    "snapshot": snapshot,
                })
            }
            Ok(ValidationOutcome::NeedConfirm(reply)) => {
                json!({ "reply": reply.text_content().unwrap_or(""), "status": "NeedConfirm", "detail": "" })
            }
            Ok(ValidationOutcome::Reject(reply)) => {
                json!({ "reply": reply.text_content().unwrap_or(""), "status": "Rejected", "detail": "" })
            }
            Ok(ValidationOutcome::Ignore) => {
                json!({ "reply": "", "status": "Ignored", "detail": "无法识别" })
            }
            Err(e) => json!({ "reply": format!("处理失败: {}", e), "status": "Error", "detail": "" }),
        }
    }

    fn build_round_config(&self) -> RoundConfig {
        let round = Round {
            round_id: self.round_id(),
            group_id: self.fixture.group_id.clone(),
            title: self.fixture.title.clone(),
            status: RoundStatus::Active,
            start_at: None,
            end_at: None,
            allow_cancel: true,
            allow_modify: true,
            default_timezone: "Asia/Shanghai".to_string(),
            created_by: "chat".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        RoundConfig {
            round,
            items: self.fixture.items.clone(),
            aliases: vec![],
            eligibility: self.eligibility.clone(),
        }
    }

    async fn rebuild(&self) -> (i64, Value) {
        let engine = ReplayEngine::new();
        let options = ReplayOptions {
            replay_id: "live".to_string(),
            include_settlement: false,
            snapshot_interval: 50,
        };
        let result = engine
            .replay(self.build_round_config(), self.events.clone(), options)
            .await;
        match result {
            Ok(r) => {
                let snap = r.final_snapshot;
                let v = snap.version;
                (v, serde_json::to_value(&snap).unwrap_or(Value::Null))
            }
            Err(_) => (0, Value::Null),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ChatRequest {
    user_id: String,
    #[serde(default)]
    nickname: Option<String>,
    text: String,
    #[serde(default)]
    is_admin: Option<bool>,
}

type SharedSession = Arc<Mutex<ChatSession>>;

async fn api_chat(State(state): State<SharedSession>, Json(req): Json<ChatRequest>) -> Json<Value> {
    let mut session = state.lock().await;
    let resp = session.handle(&req).await;
    Json(resp)
}

async fn api_snapshot(State(state): State<SharedSession>) -> Json<Value> {
    let session = state.lock().await;
    let (version, snapshot) = session.rebuild().await;
    Json(json!({ "version": version, "snapshot": snapshot }))
}

async fn api_messages(State(state): State<SharedSession>) -> Json<Value> {
    let session = state.lock().await;
    let msgs: Vec<Value> = session
        .messages
        .iter()
        .map(|m| {
            json!({
                "message_id": m.message_id,
                "user_id": m.user_id,
                "nickname": m.nickname,
                "text": m.text,
                "timestamp_ms": m.timestamp_ms,
                "is_admin": m.is_admin,
            })
        })
        .collect();
    Json(json!({ "messages": msgs }))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let mut round_config: Option<String> = None;
    let mut port: u16 = 8090;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--round-config" => {
                i += 1;
                round_config = args.get(i).cloned();
            }
            "--port" => {
                i += 1;
                port = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(8090);
            }
            other => anyhow::bail!("未知参数: {}", other),
        }
        i += 1;
    }

    let round_config = round_config.ok_or_else(|| anyhow::anyhow!("缺少 --round-config"))?;
    let fixture = load_round_fixture(Path::new(&round_config))?;

    let state: SharedSession = Arc::new(Mutex::new(ChatSession::new(fixture)));

    let app = Router::new()
        .route("/", get(index))
        .route("/api/chat", post(api_chat))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/messages", get(api_messages))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("排谷模拟聊天服务器: http://127.0.0.1:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<title>排谷模拟聊天</title>
<style>
  body{font-family:system-ui,sans-serif;margin:0;display:flex;height:100vh}
  #left{width:420px;display:flex;flex-direction:column;border-right:1px solid #ddd}
  #log{flex:1;overflow:auto;padding:8px;background:#fafafa}
  .msg{margin:4px 0;padding:6px 8px;border-radius:8px;max-width:90%;white-space:pre-wrap}
  .in{background:#e8f0fe}
  .out{background:#e6f4ea;margin-left:auto}
  .sys{background:#fff3cd;font-size:12px}
  #bar{display:flex;gap:4px;padding:8px;border-top:1px solid #ddd}
  #bar input,#bar select{padding:6px}
  #text{flex:1}
  #right{flex:1;overflow:auto;padding:12px}
  .box{font-family:monospace;margin:2px 0}
  .cell{display:inline-block;min-width:64px;padding:2px 4px;margin:1px;border:1px solid #ccc;font-size:12px;text-align:center}
  .filled{background:#d1e7dd}.locked{background:#f8d7da;color:#842029}.empty{color:#999}
</style>
</head>
<body>
<div id="left">
  <div id="log"></div>
  <div id="bar">
    <select id="user"></select>
    <input id="text" placeholder="输入排谷话术，如：排燐音吧唧2 / 包尾蓝良吧唧 / 购物金排立牌1">
    <button onclick="send()">发送</button>
  </div>
</div>
<div id="right"><h3>排结果</h3><div id="snap"></div></div>
<script>
const users=[["u_rinne","凛音"],["u_ran","蓝良推"],["u_aoi","葵"],["u_niki","仁兔"],["u_sora","空"],["u_hiyori","日和"],["u_jun","润"],["u_kanade","奏汰"],["u_mayoi","真宵"]];
const sel=document.getElementById('user');
users.forEach(u=>{const o=document.createElement('option');o.value=u[0];o.textContent=u[1];sel.appendChild(o)});
function add(cls,text){const d=document.createElement('div');d.className='msg '+cls;d.textContent=text;document.getElementById('log').appendChild(d);document.getElementById('log').scrollTop=1e9;}
async function send(){
  const text=document.getElementById('text').value; if(!text.trim())return;
  const uid=sel.value, nick=users.find(u=>u[0]===uid)[1];
  add('in',nick+'：'+text); document.getElementById('text').value='';
  const r=await fetch('/api/chat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({user_id:uid,nickname:nick,text})});
  const j=await r.json();
  add('out','bot：'+(j.reply||'（不回复）')+'  ['+j.status+']');
  if(j.detail) add('sys',j.detail);
  refresh();
}
async function refresh(){
  const r=await fetch('/api/snapshot'); const j=await r.json(); const s=j.snapshot;
  let html='<div>版本 #'+j.version+'</div>';
  if(s&&s.item_allocations){for(const ia of s.item_allocations){
    html+='<h4>'+ia.item_name+'</h4>';
    for(const b of ia.boxes){html+='<div class="box">盒'+b.box_index+'：';
      for(const sl of b.slots){let c='empty',t='·';
        if(sl.status==='filled'){c='filled';t=sl.user_id?sl.user_id.0:'?'}
        else if(sl.status==='locked_empty'){c='locked';t='LOCK'}
        html+='<span class="cell '+c+'">'+t+'</span>';}
      html+='</div>';}
    if(ia.singles.length){html+='<div>单领：'+ia.singles.map(x=>x.user_id.0+'x'+x.quantity).join(', ')+'</div>';}
    if(ia.waiting.length){html+='<div>等待：'+ia.waiting.map(x=>x.user_id.0+'x'+x.quantity).join(', ')+'</div>';}
  }}
  document.getElementById('snap').innerHTML=html;
}
document.getElementById('text').addEventListener('keydown',e=>{if(e.key==='Enter')send()});
refresh();
</script>
</body></html>
"#;
