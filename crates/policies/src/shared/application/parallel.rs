use cedar_policy::{Authorizer, Context, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tokio::time::{timeout, Duration};

/// Scenario description compatible with Cedar
#[derive(Clone, Debug)]
pub struct AuthScenario {
    pub name: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

/// mpsc-based pipeline with back-pressure and explicit worker count
pub async fn evaluate_scenarios_channel(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    workers: usize,
    buffer: usize,
) -> Result<(Vec<AuthOutcome>, ParallelStats), String> {
    let (tx_in, rx_in) = mpsc::channel::<AuthScenario>(buffer);
    let (tx_out, mut rx_out) = mpsc::channel::<AuthOutcome>(buffer);

    // clone shared inputs
    let policies = policies.clone();
    let entities = entities.clone();

    // Producer
    let scenarios_total = scenarios.len();
    tokio::spawn(async move {
        for sc in scenarios.into_iter() {
            if tx_in.send(sc).await.is_err() { break; }
        }
        // drop sender to close channel
    });

    // Workers
    let rx_arc = Arc::new(Mutex::new(rx_in));
    for _ in 0..workers {
        let rx = rx_arc.clone();
        let tx = tx_out.clone();
        let policies = policies.clone();
        let entities = entities.clone();
        tokio::spawn(async move {
            let authorizer = Authorizer::new();
            loop {
                let sc_opt = { rx.lock().await.recv().await };
                let Some(sc) = sc_opt else { break };
                let principal = match EntityUid::from_str(&sc.principal) { Ok(v) => v, Err(_) => continue };
                let action = match EntityUid::from_str(&sc.action) { Ok(v) => v, Err(_) => continue };
                let resource = match EntityUid::from_str(&sc.resource) { Ok(v) => v, Err(_) => continue };
                let context = {
                    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
                    if let Some(ctx) = sc.context.as_ref() {
                        for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } }
                    }
                    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
                };
                let request = match Request::new(principal, action, resource, context, None) { Ok(v) => v, Err(_) => continue };
                let nm = sc.name.clone();
                let nm_to = nm.clone();
                let a = &authorizer;
                let p = &policies;
                let e = &entities;
                let fut = async move {
                    let start = std::time::Instant::now();
                    let resp = a.is_authorized(&request, p, e);
                    let allow = resp.decision() == cedar_policy::Decision::Allow;
                    let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
                    let eval_time_us = start.elapsed().as_micros() as u64;
                    AuthOutcome { name: nm.clone(), allow, eval_time_us, reasons }
                };
                let outcome = if let Some(ms) = timeout_ms {
                    match timeout(Duration::from_millis(ms), fut).await {
                        Ok(o) => o,
                        Err(_elapsed) => AuthOutcome { name: nm_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] },
                    }
                } else { fut.await };
                let _ = tx.send(outcome).await;
            }
        });
    }
    // rx_in is moved into rx_arc
    drop(tx_out);

    // Collector
    let mut outcomes = Vec::new();
    let mut stats = ParallelStats { scenarios_total, ..Default::default() };
    while let Some(out) = rx_out.recv().await {
        if out.eval_time_us == 0 && out.reasons.iter().any(|r| r == "timeout") { stats.timeouts += 1; }
        stats.total_eval_time_us += out.eval_time_us;
        outcomes.push(out);
    }
    Ok((outcomes, stats))
}

/// Evaluate scenarios in parallel and return the first outcome matching the predicate.
/// Early-cancels remaining work using a shared AtomicBool.
pub async fn evaluate_until_first<F>(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    workers: usize,
    buffer: usize,
    predicate: F,
) -> Result<Option<AuthOutcome>, String>
where
    F: Fn(&AuthOutcome) -> bool + Send + Sync + 'static,
{
    let predicate = Arc::new(predicate);
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx_in, rx_in) = mpsc::channel::<AuthScenario>(buffer);
    let (tx_out, mut rx_out) = mpsc::channel::<AuthOutcome>(buffer);

    // clones
    let policies = policies.clone();
    let entities = entities.clone();

    // producer
    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            for sc in scenarios.into_iter() {
                if cancel.load(Ordering::Relaxed) { break; }
                if tx_in.send(sc).await.is_err() { break; }
            }
        }
    });

    // workers
    let rx_arc = Arc::new(Mutex::new(rx_in));
    for _ in 0..workers {
        let rx = rx_arc.clone();
        let tx = tx_out.clone();
        let policies = policies.clone();
        let entities = entities.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let authorizer = Authorizer::new();
            while !cancel.load(Ordering::Relaxed) {
                let sc_opt = { rx.lock().await.recv().await };
                let Some(sc) = sc_opt else { break };
                let principal = match EntityUid::from_str(&sc.principal) { Ok(v) => v, Err(_) => continue };
                let action = match EntityUid::from_str(&sc.action) { Ok(v) => v, Err(_) => continue };
                let resource = match EntityUid::from_str(&sc.resource) { Ok(v) => v, Err(_) => continue };
                let context = {
                    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
                    if let Some(ctx) = sc.context.as_ref() { for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } } }
                    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
                };
                let request = match Request::new(principal, action, resource, context, None) { Ok(v) => v, Err(_) => continue };
                let nm = sc.name.clone();
                let nm_to = nm.clone();
                let a = &authorizer;
                let p = &policies;
                let e = &entities;
                let fut = async move {
                    let start = std::time::Instant::now();
                    let resp = a.is_authorized(&request, p, e);
                    let allow = resp.decision() == cedar_policy::Decision::Allow;
                    let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
                    let eval_time_us = start.elapsed().as_micros() as u64;
                    AuthOutcome { name: nm, allow, eval_time_us, reasons }
                };
                let outcome = if let Some(ms) = timeout_ms {
                    match timeout(Duration::from_millis(ms), fut).await { Ok(o) => o, Err(_elapsed) => AuthOutcome { name: nm_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] } }
                } else { fut.await };
                if cancel.load(Ordering::Relaxed) { break; }
                let _ = tx.send(outcome).await;
            }
        });
    }
    // rx_in moved into rx_arc
    drop(tx_out);

    // collector
    while let Some(out) = rx_out.recv().await {
        if predicate.as_ref()(&out) {
            cancel.store(true, Ordering::Relaxed);
            return Ok(Some(out));
        }
    }
    Ok(None)
}

#[derive(Clone, Debug)]
pub struct AuthOutcome {
    pub name: String,
    pub allow: bool,
    pub eval_time_us: u64,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ParallelStats {
    pub scenarios_total: usize,
    pub timeouts: usize,
    pub total_eval_time_us: u64,
}

pub fn build_policy_set(policies: &[String]) -> Result<PolicySet, String> {
    let mut pset = PolicySet::new();
    for (i, pstr) in policies.iter().enumerate() {
        let pol: Policy = pstr
            .parse()
            .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
        pset.add(pol)
            .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
    }
    Ok(pset)
}

pub fn build_entities(defs: &[(String, HashMap<String, serde_json::Value>, Vec<String>)]) -> Result<Entities, String> {
    if defs.is_empty() { return Ok(Entities::empty()); }
    let mut out = Vec::with_capacity(defs.len());
    for (uid_str, attrs_map, parents_vec) in defs {
        let uid = EntityUid::from_str(uid_str).map_err(|e| e.to_string())?;
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        for (k, v) in attrs_map.iter() {
            if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
        }
        let mut parents: HashSet<EntityUid> = HashSet::new();
        for p in parents_vec.iter() { parents.insert(EntityUid::from_str(p).map_err(|e| e.to_string())?); }
        let ent = Entity::new(uid, attrs, parents).map_err(|e| e.to_string())?;
        out.push(ent);
    }
    Entities::from_entities(out, None).map_err(|e| e.to_string())
}

pub fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: BTreeMap<String, RestrictedExpression> = BTreeMap::new();
            for (k, val) in map.iter() { if let Some(expr) = json_to_expr(val) { rec.insert(k.clone(), expr); } }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

pub async fn evaluate_scenarios_joinset(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    max_concurrency: usize,
) -> Result<Vec<AuthOutcome>, String> {
    let mut set: JoinSet<Result<AuthOutcome, String>> = JoinSet::new();
    let mut iter = scenarios.into_iter();

    // seed
    for _ in 0..max_concurrency {
        if let Some(sc) = iter.next() { spawn_eval(&mut set, policies.clone(), entities.clone(), sc, timeout_ms); }
    }

    let mut outcomes = Vec::new();
    while let Some(joined) = set.join_next().await {
        let out = match joined { Ok(r) => r, Err(e) => Err(e.to_string()) }?;
        outcomes.push(out);
        if let Some(sc) = iter.next() { spawn_eval(&mut set, policies.clone(), entities.clone(), sc, timeout_ms); }
    }
    Ok(outcomes)
}

fn spawn_eval(
    set: &mut JoinSet<Result<AuthOutcome, String>>,
    policies: PolicySet,
    entities: Entities,
    sc: AuthScenario,
    timeout_ms: Option<u64>,
) {
    set.spawn(async move {
        let authorizer = Authorizer::new();
        let principal = EntityUid::from_str(&sc.principal).map_err(|e| e.to_string())?;
        let action = EntityUid::from_str(&sc.action).map_err(|e| e.to_string())?;
        let resource = EntityUid::from_str(&sc.resource).map_err(|e| e.to_string())?;
        let context = {
            let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
            if let Some(ctx) = sc.context.as_ref() {
                for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } }
            }
            Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
        };
        let request = Request::new(principal, action, resource, context, None).map_err(|e| e.to_string())?;
        let name = sc.name.clone();
        let name_to = name.clone();
        let fut = async move {
            let start = std::time::Instant::now();
            let resp = authorizer.is_authorized(&request, &policies, &entities);
            let allow = resp.decision() == cedar_policy::Decision::Allow;
            let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
            let eval_time_us = start.elapsed().as_micros() as u64;
            Ok(AuthOutcome { name: name.clone(), allow, eval_time_us, reasons })
        };
        if let Some(ms) = timeout_ms {
            match timeout(Duration::from_millis(ms), fut).await {
                Ok(r) => r,
                Err(_elapsed) => Ok(AuthOutcome { name: name_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] }),
            }
        } else {
            fut.await
        }
    });
}
