use super::*;

fn cand<'a>(user: &'a str, text: &'a str) -> Candidate<'a> {
    Candidate {
        user_id: user,
        text,
        is_admin: false,
        force_boundary: false,
    }
}

fn groups(cands: &[Candidate<'_>]) -> Vec<Vec<usize>> {
    group_candidates(cands, &[])
}

#[test]
fn classify_keywords_and_chatter() {
    assert_eq!(
        classify(&cand("u", "排 通行证 1"), &[]),
        Classification::Join(GroupIntent::Claim)
    );
    assert_eq!(
        classify(&cand("u", "我这个是代排"), &[]),
        Classification::Join(GroupIntent::Claim)
    );
    assert_eq!(
        classify(&cand("u", "通行证(代)"), &[]),
        Classification::Join(GroupIntent::Claim)
    );
    assert_eq!(
        classify(&cand("u", "撤 通行证"), &[]),
        Classification::Join(GroupIntent::Cancel)
    );
    assert_eq!(
        classify(&cand("u", "改 通行证 2"), &[]),
        Classification::Join(GroupIntent::Modify)
    );
    assert_eq!(
        classify(&cand("u", "取消 代排"), &[]),
        Classification::Join(GroupIntent::Cancel)
    );
    assert_eq!(
        classify(&cand("u", "今天天气不错"), &[]),
        Classification::Other
    );
    assert_eq!(
        classify(&cand("u", "想要一个悠人"), &[]),
        Classification::Other
    );
    assert_eq!(classify(&cand("u", "   "), &[]), Classification::Boundary);
    assert_eq!(classify(&cand("u", "/锁位"), &[]), Classification::Boundary);
}

#[test]
fn groups_adjacent_same_sender_same_intent() {
    let cands = vec![
        cand("u1", "排 通行证 1"),
        cand("u1", "排 人事部 1"),
        cand("u2", "排 通行证 1"),
        cand("u1", "排 通行证 1"),
    ];
    assert_eq!(groups(&cands), vec![vec![0, 1], vec![2], vec![3]]);
}

#[test]
fn interruption_and_intent_change_split() {
    let cands = vec![
        cand("u1", "排 通行证 1"),
        cand("u2", "排 通行证 1"),
        cand("u1", "排 通行证 1"),
        cand("u1", "改 通行证 2"),
    ];
    assert_eq!(groups(&cands), vec![vec![0], vec![1], vec![2], vec![3]]);
}

#[test]
fn chatter_is_singleton_and_breaks() {
    let cands = vec![
        cand("u1", "排 通行证 1"),
        cand("u1", "谢谢"),
        cand("u1", "排 人事部 1"),
    ];
    assert_eq!(groups(&cands), vec![vec![0], vec![1], vec![2]]);
}

#[test]
fn joined_text_uses_time_order_lines() {
    let cands = vec![cand("u1", "克莱门莎通行证1"), cand("u1", "我这个是代排")];
    let g = vec![0, 1];
    assert_eq!(joined_text(&cands, &g), "克莱门莎通行证1\n我这个是代排");
}
