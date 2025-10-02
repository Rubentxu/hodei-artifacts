use policies::shared::application::parallel::{
    AuthScenario, build_policy_set, build_entities, evaluate_scenarios_channel, evaluate_until_first
};

#[tokio::test]
async fn channel_evaluates_multiple_scenarios() {
    let pset = build_policy_set(&vec![
        "permit(principal, action, resource) when { context.mfa == true };".to_string()
    ]).expect("policy set");
    let ents = build_entities(&[]).expect("entities");

    let scenarios = vec![
        AuthScenario { name: "s1".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"view\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(std::iter::once(("mfa".to_string(), serde_json::json!(true))).collect()) },
        AuthScenario { name: "s2".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"view\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(std::iter::once(("mfa".to_string(), serde_json::json!(true))).collect()) },
    ];

    let (outcomes, stats) = evaluate_scenarios_channel(&pset, &ents, scenarios, None, 4, 8).await.expect("run");
    assert_eq!(outcomes.len(), 2);
    assert_eq!(stats.scenarios_total, 2);
    assert!(outcomes.iter().all(|o| o.allow));
}

#[tokio::test]
async fn until_first_returns_on_first_allow() {
    let pset = build_policy_set(&vec![
        "permit(principal, action, resource) when { context.allowed == true };".to_string()
    ]).expect("policy set");
    let ents = build_entities(&[]).expect("entities");

    let mut ctx_deny = std::collections::HashMap::new();
    ctx_deny.insert("allowed".to_string(), serde_json::json!(false));
    let mut ctx_allow = std::collections::HashMap::new();
    ctx_allow.insert("allowed".to_string(), serde_json::json!(true));

    let scenarios = vec![
        AuthScenario { name: "deny".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"a\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(ctx_deny) },
        AuthScenario { name: "allow".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"a\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(ctx_allow) },
    ];

    let first = evaluate_until_first(&pset, &ents, scenarios, None, 2, 4, |o| o.allow).await.expect("run");
    assert!(first.is_some());
    assert_eq!(first.unwrap().name, "allow");
}
