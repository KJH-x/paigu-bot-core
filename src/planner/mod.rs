#![allow(dead_code)]

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::domain::snapshot::AllocationSnapshot;
use crate::settlement::{
    evaluate, Line, OrderTable, Package, ScopeMode, SettlementConfig, SettlementResult, UnitPrice,
};

const MAX_CANDIDATES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    GiftMax,
    DiscountMax,
}

impl Default for Strategy {
    fn default() -> Self {
        Strategy::GiftMax
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanLimits {
    #[serde(default = "default_max_packages")]
    pub max_packages: u32,
    #[serde(default = "default_max_iters")]
    pub max_iters: u64,
    #[serde(default = "default_time_budget_ms")]
    pub time_budget_ms: u64,
}

fn default_max_packages() -> u32 {
    8
}
fn default_max_iters() -> u64 {
    50_000
}
fn default_time_budget_ms() -> u64 {
    250
}

impl Default for PlanLimits {
    fn default() -> Self {
        Self {
            max_packages: default_max_packages(),
            max_iters: default_max_iters(),
            time_budget_ms: default_time_budget_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRequest {
    pub config: SettlementConfig,
    pub allocation: AllocationSnapshot,
    pub strategy: Strategy,
    pub limits: PlanLimits,
    #[serde(default)]
    pub prices: Vec<UnitPrice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanScore {
    pub discount_total: i64,
    pub highest_tier_hits: u32,
    pub gift_count: u32,
    pub gift_valuation_total: i64,
    pub grand_total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCandidate {
    pub table: OrderTable,
    pub result: SettlementResult,
    pub score: PlanScore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStats {
    pub atoms: usize,
    pub nodes: u64,
    pub evaluated: u64,
    pub pruned: u64,
    pub packages_used: usize,
    pub truncated: bool,
    pub truncation_reason: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResult {
    pub best: OrderTable,
    pub best_result: SettlementResult,
    pub best_score: PlanScore,
    pub candidates: Vec<PlanCandidate>,
    pub stats: PlanStats,
}

#[derive(Debug, Clone)]
struct Atom {
    item_id: String,
    variant_id: Option<String>,
    is_gift: bool,
    qty: u32,
    unit_price_cents: i64,
}

pub fn plan(req: &PlanRequest) -> PlanResult {
    let atoms = collect_atoms(req);
    let total_gross = atoms
        .iter()
        .filter(|a| !a.is_gift)
        .map(|a| a.unit_price_cents.saturating_mul(a.qty as i64))
        .fold(0i64, i64::saturating_add);
    let max_packages = req.limits.max_packages.max(1) as usize;

    let mut search = Search {
        req,
        atoms,
        total_gross,
        max_packages,
        best: None,
        candidates: Vec::new(),
        nodes: 0,
        evaluated: 0,
        pruned: 0,
        truncated: false,
        truncation_reason: None,
        start: Instant::now(),
    };
    let mut packages: Vec<Vec<usize>> = Vec::new();
    search.dfs(0, &mut packages);
    let elapsed_ms = search.start.elapsed().as_millis() as u64;

    let (best_score, best, best_result) = match search.best.take() {
        Some(found) => found,
        None => {
            let table = OrderTable::new(Vec::new());
            let result = evaluate(&req.config, &table);
            let score = score_of(&req.config, &result);
            (score, table, result)
        }
    };

    let mut candidates = search.candidates;
    candidates.sort_by(|a, b| score_cmp(&b.score, &a.score, req.strategy));

    let stats = PlanStats {
        atoms: search.atoms.len(),
        nodes: search.nodes,
        evaluated: search.evaluated,
        pruned: search.pruned,
        packages_used: best.packages.len(),
        truncated: search.truncated,
        truncation_reason: search.truncation_reason,
        elapsed_ms,
    };

    PlanResult {
        best,
        best_result,
        best_score,
        candidates,
        stats,
    }
}

pub fn manual_evaluate(config: &SettlementConfig, table: &OrderTable) -> SettlementResult {
    evaluate(config, table)
}

struct Search<'a> {
    req: &'a PlanRequest,
    atoms: Vec<Atom>,
    total_gross: i64,
    max_packages: usize,
    best: Option<(PlanScore, OrderTable, SettlementResult)>,
    candidates: Vec<PlanCandidate>,
    nodes: u64,
    evaluated: u64,
    pruned: u64,
    truncated: bool,
    truncation_reason: Option<String>,
    start: Instant,
}

impl<'a> Search<'a> {
    fn dfs(&mut self, atom_idx: usize, packages: &mut Vec<Vec<usize>>) {
        if self.truncated {
            return;
        }
        if self.nodes >= self.req.limits.max_iters {
            self.truncated = true;
            self.truncation_reason = Some("max_iters".to_string());
            return;
        }
        if self.start.elapsed().as_millis() as u64 > self.req.limits.time_budget_ms {
            self.truncated = true;
            self.truncation_reason = Some("time_budget".to_string());
            return;
        }
        self.nodes += 1;

        if atom_idx == self.atoms.len() {
            self.leaf(packages);
            return;
        }

        let k = packages.len();
        for p in 0..=k {
            if p == k {
                if k >= self.max_packages {
                    break;
                }
                packages.push(Vec::new());
            }
            packages[p].push(atom_idx);

            let prune = match &self.best {
                Some((best_score, _, _)) => {
                    let bound = self.bound(packages, atom_idx + 1);
                    score_cmp(&bound, best_score, self.req.strategy) != Ordering::Greater
                }
                None => false,
            };

            if prune {
                self.pruned += 1;
            } else {
                self.dfs(atom_idx + 1, packages);
            }

            packages[p].pop();
            if p == k {
                packages.pop();
            }
            if self.truncated {
                return;
            }
        }
    }

    fn leaf(&mut self, packages: &[Vec<usize>]) {
        let table = self.build_table(packages);
        let result = evaluate(&self.req.config, &table);
        let score = score_of(&self.req.config, &result);
        self.evaluated += 1;

        let better = match &self.best {
            None => true,
            Some((best_score, _, _)) => {
                score_cmp(&score, best_score, self.req.strategy) == Ordering::Greater
            }
        };
        if better {
            self.best = Some((score.clone(), table.clone(), result.clone()));
        }
        self.push_candidate(PlanCandidate {
            table,
            result,
            score,
        });
    }

    fn push_candidate(&mut self, candidate: PlanCandidate) {
        if self.candidates.len() < MAX_CANDIDATES {
            self.candidates.push(candidate);
            return;
        }
        let mut worst = 0usize;
        for i in 1..self.candidates.len() {
            if score_cmp(
                &self.candidates[i].score,
                &self.candidates[worst].score,
                self.req.strategy,
            ) == Ordering::Less
            {
                worst = i;
            }
        }
        if score_cmp(
            &candidate.score,
            &self.candidates[worst].score,
            self.req.strategy,
        ) == Ordering::Greater
        {
            self.candidates[worst] = candidate;
        }
    }

    fn bound(&self, packages: &[Vec<usize>], next_atom: usize) -> PlanScore {
        let remaining: Vec<usize> = (next_atom..self.atoms.len()).collect();
        let mut synthetic: Vec<Vec<usize>> = Vec::with_capacity(self.max_packages);
        for pkg in packages {
            let mut lines = pkg.clone();
            lines.extend_from_slice(&remaining);
            synthetic.push(lines);
        }
        while synthetic.len() < self.max_packages {
            synthetic.push(remaining.clone());
        }
        let table = self.build_table(&synthetic);
        let result = evaluate(&self.req.config, &table);
        let mut score = score_of(&self.req.config, &result);
        score.grand_total = if self.req.strategy == Strategy::GiftMax {
            self.total_gross
        } else {
            0
        };
        score
    }

    fn build_table(&self, packages: &[Vec<usize>]) -> OrderTable {
        let include_gift = matches!(self.req.config.scope_mode, ScopeMode::IncludeGift);
        let mut built: Vec<(i64, usize, Package)> = packages
            .iter()
            .enumerate()
            .map(|(idx, atom_ids)| {
                // 合并键不含价格/购买人：同一商品（含不同人买的）只记一种（A-3）
                let mut merged: BTreeMap<(String, Option<String>, bool), (u32, i64)> =
                    BTreeMap::new();
                for &ai in atom_ids {
                    let atom = &self.atoms[ai];
                    let entry = merged
                        .entry((atom.item_id.clone(), atom.variant_id.clone(), atom.is_gift))
                        .or_insert((0, atom.unit_price_cents));
                    entry.0 = entry.0.saturating_add(atom.qty);
                }
                let lines: Vec<Line> = merged
                    .into_iter()
                    .map(
                        |((item_id, variant_id, is_gift), (qty, unit_price_cents))| Line {
                            item_id,
                            variant_id,
                            qty,
                            unit_price_cents,
                            is_gift,
                        },
                    )
                    .collect();
                let mut gross = 0i64;
                let mut gift = 0i64;
                for line in &lines {
                    let total = line.total_cents();
                    if line.is_gift {
                        gift = gift.saturating_add(total);
                    } else {
                        gross = gross.saturating_add(total);
                    }
                }
                let scope = if include_gift {
                    gross.saturating_add(gift)
                } else {
                    gross
                };
                (
                    scope,
                    lines.len(),
                    Package {
                        package_id: format!("pkg-{}", idx + 1),
                        lines,
                    },
                )
            })
            .collect();
        built.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(a.2.package_id.cmp(&b.2.package_id))
        });
        OrderTable::new(built.into_iter().map(|(_, _, pkg)| pkg).collect())
    }
}

fn score_of(config: &SettlementConfig, result: &SettlementResult) -> PlanScore {
    let highest_threshold = config.gift_tiers.iter().map(|t| t.threshold).max();
    let highest_tier_hits = match highest_threshold {
        Some(threshold) => result
            .packages
            .iter()
            .filter(|p| p.gross_cents >= threshold)
            .count() as u32,
        None => 0,
    };
    let gift_count = result
        .packages
        .iter()
        .map(|p| p.gift_count)
        .fold(0u32, u32::saturating_add);
    PlanScore {
        discount_total: result.discount_total,
        highest_tier_hits,
        gift_count,
        gift_valuation_total: result.gift_valuation_total,
        grand_total: result.grand_total,
    }
}

fn score_cmp(a: &PlanScore, b: &PlanScore, strategy: Strategy) -> Ordering {
    match strategy {
        Strategy::GiftMax => a
            .highest_tier_hits
            .cmp(&b.highest_tier_hits)
            .then(a.gift_count.cmp(&b.gift_count))
            .then(a.gift_valuation_total.cmp(&b.gift_valuation_total))
            .then(a.grand_total.cmp(&b.grand_total)),
        Strategy::DiscountMax => a
            .discount_total
            .cmp(&b.discount_total)
            .then(a.gift_count.cmp(&b.gift_count))
            .then(a.gift_valuation_total.cmp(&b.gift_valuation_total)),
    }
}

fn collect_atoms(req: &PlanRequest) -> Vec<Atom> {
    let mut atoms = Vec::new();
    for alloc in &req.allocation.item_allocations {
        let is_gift = alloc.kind == "gift";
        let variant = alloc.variant_id.clone();
        // 标价一律取商品目录（canonical）：同一商品不因购买人/单领自带价而拆成两种（A-3）
        let base = lookup_price(&req.prices, &alloc.item_id.0, variant.as_deref());

        for mbox in &alloc.boxes {
            for slot in &mbox.slots {
                if slot.user_id.is_some() {
                    atoms.push(Atom {
                        item_id: alloc.item_id.0.clone(),
                        variant_id: variant.clone(),
                        is_gift,
                        qty: 1,
                        unit_price_cents: base,
                    });
                }
            }
        }

        for single in &alloc.singles {
            for _ in 0..single.quantity {
                atoms.push(Atom {
                    item_id: alloc.item_id.0.clone(),
                    variant_id: variant.clone(),
                    is_gift,
                    qty: 1,
                    unit_price_cents: base,
                });
            }
        }
    }
    atoms.sort_by(|a, b| {
        a.item_id
            .cmp(&b.item_id)
            .then(a.variant_id.cmp(&b.variant_id))
            .then(a.is_gift.cmp(&b.is_gift))
            .then(a.unit_price_cents.cmp(&b.unit_price_cents))
    });
    atoms
}

fn lookup_price(prices: &[UnitPrice], item_id: &str, variant_id: Option<&str>) -> i64 {
    if let Some(variant) = variant_id {
        if let Some(found) = prices
            .iter()
            .find(|p| p.item_id == item_id && p.variant_id.as_deref() == Some(variant))
        {
            return found.unit_price_cents;
        }
    }
    prices
        .iter()
        .find(|p| p.item_id == item_id && p.variant_id.is_none())
        .map(|p| p.unit_price_cents)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
