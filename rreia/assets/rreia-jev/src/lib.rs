//! RREIA-Jev: servidor local de decisões tipadas, no formato da API System One
//! do Jev (POST /v1/systemone, perguntas `choice`, `score` e `noul`), sobre
//! qualquer modelo local com API compatível com a OpenAI (llama-server,
//! mistral.rs, Ollama). Acrescenta admissão por hardware (L1 a L3), fila de
//! prioridade com permissões por chave (L4) e rastro de auditoria (L6).

use serde_json::{json, Map, Value};
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    io::Read,
    sync::{mpsc, Arc, Condvar, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// ---------------------------------------------------------------- hardware

/// Classes de prioridade (L4). Na ordem derivada, P0 é a maior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    P3,
    P2,
    P1,
    P0,
}

impl Class {
    pub fn parse(s: &str) -> Option<Class> {
        match s.trim().to_ascii_uppercase().as_str() {
            "P0" => Some(Class::P0),
            "P1" => Some(Class::P1),
            "P2" => Some(Class::P2),
            "P3" => Some(Class::P3),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Class::P0 => "P0",
            Class::P1 => "P1",
            Class::P2 => "P2",
            Class::P3 => "P3",
        }
    }
    /// Tempo limite por classe. Valores de exemplo, a calibrar (Seção 9).
    pub fn timeout(self) -> Duration {
        Duration::from_secs(match self {
            Class::P0 => 30,
            Class::P1 => 60,
            Class::P2 => 120,
            Class::P3 => 600,
        })
    }
}

pub struct Hardware {
    pub ram_gb: f64,
    pub bandwidth_gbs: f64, // B
    pub reserve_gb: f64,    // sistema operacional e aplicação
}

pub struct Policy {
    pub mem_fraction: f64, // f (L1)
    pub eta: f64,          // η (L2)
}

pub struct ModelSpec {
    pub name: String,
    pub weights_gb: f64, // M_pesos
    pub active_gb: f64,  // M_ativa (igual a M_pesos em modelo denso)
    pub layers: u32,     // L
    pub kv_heads: u32,   // n_kv
    pub head_dim: u32,   // d_head
}

impl ModelSpec {
    /// 2 · L · n_kv · d_head · s, com s = 2 bytes (FP16).
    pub fn kv_bytes_per_token(&self) -> f64 {
        2.0 * self.layers as f64 * self.kv_heads as f64 * self.head_dim as f64 * 2.0
    }
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    Accept { max_ctx_tokens: u64, tok_s_floor: f64 },
    RejectMemory { budget_gb: f64 },
    RejectSpeed { tok_s_floor: f64, required: f64 },
}

/// L1, L2 e L3: decide se o modelo entra no hardware e com quanto contexto.
pub fn admit(hw: &Hardware, m: &ModelSpec, p: &Policy, min_tok_s: f64) -> Decision {
    // L1: memória disponível para o KV cache depois de pesos e reserva.
    let budget_gb = p.mem_fraction * hw.ram_gb - m.weights_gb - hw.reserve_gb;
    if budget_gb <= 0.0 {
        return Decision::RejectMemory { budget_gb };
    }
    // L2: piso de velocidade = η · B / M_ativa.
    let tok_s_floor = p.eta * hw.bandwidth_gbs / m.active_gb;
    if tok_s_floor < min_tok_s {
        return Decision::RejectSpeed { tok_s_floor, required: min_tok_s };
    }
    // L3: contexto máximo que o orçamento comporta.
    let max_ctx_tokens = (budget_gb * 1e9 / m.kv_bytes_per_token()) as u64;
    Decision::Accept { max_ctx_tokens, tok_s_floor }
}

// ------------------------------------------------------------------- fila

struct Ranked<T> {
    class: Class,
    seq: u64,
    item: T,
}

impl<T> PartialEq for Ranked<T> {
    fn eq(&self, o: &Self) -> bool {
        self.class == o.class && self.seq == o.seq
    }
}
impl<T> Eq for Ranked<T> {}
impl<T> PartialOrd for Ranked<T> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<T> Ord for Ranked<T> {
    /// Maior classe primeiro; na mesma classe, o mais antigo primeiro.
    fn cmp(&self, o: &Self) -> Ordering {
        self.class.cmp(&o.class).then(o.seq.cmp(&self.seq))
    }
}

/// Fila de prioridade (L4). A preempção acontece entre requisições:
/// uma geração em andamento nunca é interrompida.
pub struct Scheduler<T> {
    heap: BinaryHeap<Ranked<T>>,
    next: u64,
}

impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Scheduler { heap: BinaryHeap::new(), next: 0 }
    }
}

impl<T> Scheduler<T> {
    pub fn push(&mut self, class: Class, item: T) {
        self.heap.push(Ranked { class, seq: self.next, item });
        self.next += 1;
    }
    pub fn pop(&mut self) -> Option<(Class, T)> {
        self.heap.pop().map(|r| (r.class, r.item))
    }
    pub fn len(&self) -> usize {
        self.heap.len()
    }
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

/// Fila compartilhada entre as threads HTTP (produtoras) e o worker (consumidor).
pub struct SharedQueue<T> {
    inner: Mutex<Scheduler<T>>,
    ready: Condvar,
    cap: usize,
}

impl<T> SharedQueue<T> {
    pub fn new(cap: usize) -> Self {
        SharedQueue { inner: Mutex::new(Scheduler::default()), ready: Condvar::new(), cap }
    }
    /// Devolve o item se a fila estiver cheia.
    pub fn try_push(&self, class: Class, item: T) -> Result<(), T> {
        let mut q = self.inner.lock().unwrap();
        if q.len() >= self.cap {
            return Err(item);
        }
        q.push(class, item);
        self.ready.notify_one();
        Ok(())
    }
    pub fn pop_blocking(&self) -> (Class, T) {
        let mut q = self.inner.lock().unwrap();
        loop {
            if let Some(x) = q.pop() {
                return x;
            }
            q = self.ready.wait(q).unwrap();
        }
    }
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ------------------------------------------------------------ modelo local

pub struct Reply {
    pub text: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency: Duration,
}

/// Chamada ao endpoint /v1/chat/completions de um servidor local.
pub fn complete(
    base_url: &str,
    model: &str,
    prompt: &str,
    max_tokens: u32,
    timeout: Duration,
) -> Result<Reply, String> {
    let t0 = Instant::now();
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": max_tokens,
        "temperature": 0,
        "stream": false
    });
    let resp: Value = ureq::post(&format!("{base_url}/v1/chat/completions"))
        .timeout(timeout)
        .send_json(body)
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let text = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("resposta sem conteúdo")?
        .to_string();
    Ok(Reply {
        text,
        prompt_tokens: resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: resp["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        latency: t0.elapsed(),
    })
}

/// Estimativa conservadora: 1 token a cada 3 caracteres.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(3)
}

// -------------------------------------------------------- perguntas tipadas

pub enum Kind {
    /// Opções com descrição (pode ser vazia), na ordem do objeto `criteria`.
    Choice(Vec<(String, String)>),
    /// Níveis ordenados do menor para o maior.
    Score(Vec<String>),
    /// Afirmação a julgar; o texto é o critério opcional.
    Noul(String),
}

pub struct Question {
    pub id: String,
    pub instructions: String,
    pub kind: Kind,
}

impl Question {
    fn labels(&self) -> Vec<String> {
        match &self.kind {
            Kind::Choice(o) => o.iter().map(|(k, _)| k.clone()).collect(),
            Kind::Score(l) => l.clone(),
            Kind::Noul(_) => vec!["verdadeiro".into(), "falso".into()],
        }
    }
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) if a.iter().all(Value::is_string) => {
            a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")
        }
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// Valida o objeto `questions` do pedido e o converte em perguntas tipadas.
pub fn parse_questions(v: &Value) -> Result<Vec<Question>, String> {
    let qs = v
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or("`questions` deve ser um objeto com ao menos uma pergunta")?;
    qs.iter()
        .map(|(id, q)| {
            if q["instructions"].is_null() {
                return Err(format!("pergunta `{id}`: falta `instructions`"));
            }
            let instructions = as_text(&q["instructions"]);
            let criteria = &q["criteria"];
            let kind = match q["type"].as_str() {
                Some("choice") => {
                    let m = criteria.as_object().filter(|m| m.len() >= 2).ok_or_else(|| {
                        format!("pergunta `{id}`: `criteria` deve ser um objeto com 2 ou mais opções")
                    })?;
                    Kind::Choice(
                        m.iter()
                            .map(|(k, d)| (k.clone(), d.as_str().unwrap_or("").to_string()))
                            .collect(),
                    )
                }
                Some("score") => {
                    let a = criteria.as_array().filter(|a| a.len() >= 2).ok_or_else(|| {
                        format!("pergunta `{id}`: `criteria` deve ser uma lista com 2 ou mais níveis")
                    })?;
                    let levels = a
                        .iter()
                        .map(|l| {
                            l.as_str()
                                .or_else(|| l["label"].as_str())
                                .map(String::from)
                                .ok_or_else(|| format!("pergunta `{id}`: nível sem texto"))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Kind::Score(levels)
                }
                Some("noul") => Kind::Noul(if criteria.is_null() { String::new() } else { as_text(criteria) }),
                Some(other) => return Err(format!("pergunta `{id}`: tipo `{other}` desconhecido")),
                None => return Err(format!("pergunta `{id}`: falta `type`")),
            };
            Ok(Question { id: id.clone(), instructions, kind })
        })
        .collect()
}

pub fn build_prompt(state: &str, qs: &[Question]) -> String {
    let mut p = String::from(
        "Leia o ESTADO e responda cada PERGUNTA com uma distribuição de probabilidade sobre os candidatos.\n\nESTADO:\n",
    );
    p.push_str(state);
    p.push_str("\n\nPERGUNTAS:\n");
    for q in qs {
        p.push_str(&format!("[{}] {}\n", q.id, q.instructions));
        let cands: Vec<String> = match &q.kind {
            Kind::Choice(o) => o
                .iter()
                .enumerate()
                .map(|(i, (k, d))| if d.is_empty() { format!("{i}={k}") } else { format!("{i}={k} ({d})") })
                .collect(),
            Kind::Score(l) => l.iter().enumerate().map(|(i, s)| format!("{i}={s}")).collect(),
            Kind::Noul(c) => {
                let mut v = vec!["0=verdadeiro".to_string(), "1=falso".to_string()];
                if !c.is_empty() {
                    v.push(format!("critério: {c}"));
                }
                v
            }
        };
        p.push_str(&format!("   Candidatos, em ordem: {}\n", cands.join(" | ")));
    }
    p.push_str(
        r#"
Responda só com JSON no formato {"answers":{"<id>":[p0,p1,...]}}, com um vetor por pergunta e um número não negativo por candidato, na ordem listada."#,
    );
    p
}

/// Valida um vetor de probabilidades e o normaliza para somar 1.
fn normalize(raw: &[Value], n: usize) -> Option<Vec<f64>> {
    let p: Vec<f64> = raw.iter().map(Value::as_f64).collect::<Option<_>>()?;
    if p.len() != n || p.iter().any(|x| !x.is_finite() || *x < 0.0) {
        return None;
    }
    let s: f64 = p.iter().sum();
    (s > 0.0).then(|| p.iter().map(|x| x / s).collect())
}

/// Lê a resposta do modelo: um vetor de probabilidades por pergunta.
pub fn parse_answers(text: &str, qs: &[Question]) -> Option<Vec<Vec<f64>>> {
    let (a, b) = (text.find('{')?, text.rfind('}')?);
    let v: Value = serde_json::from_str(text.get(a..=b)?).ok()?;
    let obj = v["answers"].as_object()?;
    qs.iter().map(|q| normalize(obj.get(&q.id)?.as_array()?, q.labels().len())).collect()
}

/// Confiança por entropia: 1 − H(p) / ln(n). Vale 0 se uniforme e 1 se certeza.
pub fn confidence(p: &[f64]) -> f64 {
    let h: f64 = -p.iter().filter(|x| **x > 0.0).map(|x| x * x.ln()).sum::<f64>();
    1.0 - h / (p.len() as f64).ln()
}

fn r4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

/// Monta a resposta de uma pergunta no formato do Jev.
pub fn answer(q: &Question, p: &[f64]) -> Value {
    let conf = r4(confidence(p));
    match &q.kind {
        Kind::Choice(opts) => {
            let best = (0..p.len()).max_by(|&i, &j| p[i].total_cmp(&p[j])).unwrap_or(0);
            let probs: Map<String, Value> =
                opts.iter().zip(p).map(|((k, _), x)| (k.clone(), json!(r4(*x)))).collect();
            json!({ "type": "choice", "choice": opts[best].0, "probabilities": probs, "confidence": conf })
        }
        Kind::Score(levels) => {
            let score: f64 = p.iter().enumerate().map(|(i, x)| i as f64 * x).sum();
            let legend: Map<String, Value> =
                levels.iter().enumerate().map(|(i, l)| (i.to_string(), json!(l))).collect();
            let probs: Map<String, Value> =
                p.iter().enumerate().map(|(i, x)| (i.to_string(), json!(r4(*x)))).collect();
            json!({ "type": "score", "score": r4(score), "legend": legend, "probabilities": probs, "confidence": conf })
        }
        Kind::Noul(_) => json!({ "type": "noul", "noul": r4(p[0]) }),
    }
}

// ----------------------------------------------------------------- servidor

pub struct Config {
    pub model: String,
    pub upstream: String,
    pub max_ctx_tokens: u64,
    pub max_queue: usize,
    pub attempts: u32,
    /// Chave de acesso e a maior classe que ela pode pedir. Vazio = servidor aberto.
    pub keys: Vec<(String, Class)>,
}

struct HttpErr {
    status: u16,
    msg: String,
}

impl HttpErr {
    fn new(status: u16, msg: impl Into<String>) -> Self {
        HttpErr { status, msg: msg.into() }
    }
}

struct Outcome {
    answers: Map<String, Value>,
    input_tokens: u64,
    output_tokens: u64,
    attempts: u32,
    queue_ms: u128,
    latency_ms: u128,
}

struct Work {
    class: Class,
    state: String,
    questions: Vec<Question>,
    enqueued: Instant,
    reply: mpsc::Sender<Result<Outcome, HttpErr>>,
}

fn run(cfg: &Config, w: &Work) -> Result<Outcome, HttpErr> {
    let queue_ms = w.enqueued.elapsed().as_millis();
    let t0 = Instant::now();
    let prompt = build_prompt(&w.state, &w.questions);
    let candidates: usize = w.questions.iter().map(|q| q.labels().len()).sum();
    let max_tokens = (32 + 12 * candidates).min(512) as u32;
    let (mut input_tokens, mut output_tokens) = (0, 0);
    for attempt in 1..=cfg.attempts {
        let r = complete(&cfg.upstream, &cfg.model, &prompt, max_tokens, w.class.timeout())
            .map_err(|e| HttpErr::new(502, format!("modelo local: {e}")))?;
        input_tokens += if r.prompt_tokens > 0 { r.prompt_tokens } else { estimate_tokens(&prompt) };
        output_tokens += r.completion_tokens;
        if let Some(dist) = parse_answers(&r.text, &w.questions) {
            let answers = w.questions.iter().zip(&dist).map(|(q, p)| (q.id.clone(), answer(q, p))).collect();
            return Ok(Outcome {
                answers,
                input_tokens,
                output_tokens,
                attempts: attempt,
                queue_ms,
                latency_ms: t0.elapsed().as_millis(),
            });
        }
    }
    Err(HttpErr::new(502, "o modelo não devolveu probabilidades válidas"))
}

/// Um único worker: o modelo local atende uma geração por vez. A fila decide
/// quem é o próximo (L4). O rastro (L6) não registra o conteúdo do estado.
fn worker(queue: Arc<SharedQueue<Work>>, cfg: Arc<Config>) {
    loop {
        let (class, w) = queue.pop_blocking();
        let result = run(&cfg, &w);
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let line = match &result {
            Ok(o) => json!({ "ts": ts, "class": class.name(), "outcome": "ok", "questions": w.questions.len(),
                "queue_ms": o.queue_ms, "latency_ms": o.latency_ms, "attempts": o.attempts,
                "input_tokens": o.input_tokens, "output_tokens": o.output_tokens }),
            Err(e) => json!({ "ts": ts, "class": class.name(), "outcome": format!("erro {}: {}", e.status, e.msg),
                "questions": w.questions.len() }),
        };
        println!("{line}");
        let _ = w.reply.send(result);
    }
}

type Reply3 = (u16, Value, Vec<(String, String)>);

fn header(req: &tiny_http::Request, name: &'static str) -> Option<String> {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string())
}

fn systemone(req: &mut tiny_http::Request, q: &SharedQueue<Work>, cfg: &Config) -> Reply3 {
    let err = |c: u16, m: &str| -> Reply3 { (c, json!({ "error": m }), vec![]) };

    // Permissão: a chave define a maior classe que o cliente pode pedir.
    let key_max = if cfg.keys.is_empty() {
        Class::P0
    } else {
        let bearer = header(req, "authorization")
            .and_then(|v| v.strip_prefix("Bearer ").map(str::to_string))
            .unwrap_or_default();
        match cfg.keys.iter().find(|(k, _)| *k == bearer) {
            Some((_, c)) => *c,
            None => return err(401, "chave inválida ou ausente"),
        }
    };
    let asked = header(req, "x-rreia-class").and_then(|v| Class::parse(&v)).unwrap_or(Class::P2);
    let class = asked.min(key_max);

    let mut raw = String::new();
    if req.as_reader().take(1_000_000).read_to_string(&mut raw).is_err() {
        return err(400, "corpo ilegível");
    }
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return err(400, &format!("JSON inválido: {e}")),
    };
    if v["state"].is_null() {
        return err(400, "falta `state`");
    }
    let questions = match parse_questions(&v["questions"]) {
        Ok(qs) => qs,
        Err(e) => return err(400, &e),
    };
    let state = as_text(&v["state"]);

    // L3: recusa a entrada que não cabe no contexto do orçamento.
    let est = estimate_tokens(&build_prompt(&state, &questions));
    if est > cfg.max_ctx_tokens {
        return err(413, &format!("entrada estimada em {est} tokens passa do máximo de {} (L3)", cfg.max_ctx_tokens));
    }

    let (tx, rx) = mpsc::channel();
    let work = Work { class, state, questions, enqueued: Instant::now(), reply: tx };
    if q.try_push(class, work).is_err() {
        return err(503, "fila cheia");
    }
    match rx.recv_timeout(class.timeout()) {
        Ok(Ok(o)) => (
            200,
            json!({ "model": cfg.model, "answers": o.answers,
                    "usage": { "input_tokens": o.input_tokens, "output_tokens": o.output_tokens } }),
            vec![
                ("X-RREIA-Class".into(), class.name().into()),
                ("X-RREIA-Queue-Ms".into(), o.queue_ms.to_string()),
                ("X-RREIA-Attempts".into(), o.attempts.to_string()),
            ],
        ),
        Ok(Err(e)) => (e.status, json!({ "error": e.msg }), vec![]),
        Err(_) => err(504, "tempo limite"),
    }
}

fn handle(mut req: tiny_http::Request, q: Arc<SharedQueue<Work>>, cfg: Arc<Config>) {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let (status, body, extra) = match (req.method(), path.as_str()) {
        (&tiny_http::Method::Get, "/ready") => (
            200,
            json!({ "ready": true, "model": cfg.model, "max_ctx_tokens": cfg.max_ctx_tokens, "queue": q.len() }),
            vec![],
        ),
        (&tiny_http::Method::Post, "/v1/systemone") => systemone(&mut req, &q, &cfg),
        _ => (404, json!({ "error": "rota não encontrada" }), vec![]),
    };
    let mut resp = tiny_http::Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    for (k, v) in extra {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp = resp.with_header(h);
        }
    }
    let _ = req.respond(resp);
}

/// Atende requisições até o servidor ser encerrado.
pub fn serve(server: tiny_http::Server, cfg: Config) {
    let cfg = Arc::new(cfg);
    let queue = Arc::new(SharedQueue::new(cfg.max_queue));
    {
        let (q, c) = (queue.clone(), cfg.clone());
        std::thread::spawn(move || worker(q, c));
    }
    for req in server.incoming_requests() {
        let (q, c) = (queue.clone(), cfg.clone());
        std::thread::spawn(move || handle(req, q, c));
    }
}

// ------------------------------------------------------------------- testes

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering as AO};

    fn llama8b() -> ModelSpec {
        ModelSpec { name: "llama-3.1-8b-q4_k_m".into(), weights_gb: 4.9, active_gb: 4.9, layers: 32, kv_heads: 8, head_dim: 128 }
    }
    const POL: Policy = Policy { mem_fraction: 0.75, eta: 0.5 };

    // ---- L1, L2, L3

    #[test]
    fn kv_cache_do_llama_8b_e_131072_bytes_por_token() {
        assert_eq!(llama8b().kv_bytes_per_token(), 131_072.0);
    }

    #[test]
    fn pc_de_16gb_e_aceito_com_cerca_de_38_mil_tokens() {
        let hw = Hardware { ram_gb: 16.0, bandwidth_gbs: 51.2, reserve_gb: 2.0 };
        match admit(&hw, &llama8b(), &POL, 5.0) {
            Decision::Accept { max_ctx_tokens, tok_s_floor } => {
                assert_eq!(max_ctx_tokens, 38_909);
                assert!((tok_s_floor - 5.22).abs() < 0.01);
            }
            d => panic!("esperava Accept, veio {d:?}"),
        }
    }

    #[test]
    fn pc_de_8gb_e_recusado_por_memoria() {
        let hw = Hardware { ram_gb: 8.0, bandwidth_gbs: 51.2, reserve_gb: 2.0 };
        match admit(&hw, &llama8b(), &POL, 5.0) {
            Decision::RejectMemory { budget_gb } => assert!((budget_gb + 0.9).abs() < 1e-9),
            d => panic!("esperava RejectMemory, veio {d:?}"),
        }
    }

    #[test]
    fn memoria_lenta_e_recusada_por_velocidade() {
        let hw = Hardware { ram_gb: 16.0, bandwidth_gbs: 25.0, reserve_gb: 2.0 };
        assert!(matches!(admit(&hw, &llama8b(), &POL, 5.0), Decision::RejectSpeed { .. }));
    }

    // ---- L4

    #[test]
    fn fila_atende_a_maior_classe_e_depois_o_mais_antigo() {
        let mut s = Scheduler::default();
        s.push(Class::P3, "lote");
        s.push(Class::P2, "padrao-1");
        s.push(Class::P0, "critico");
        s.push(Class::P2, "padrao-2");
        let ordem: Vec<_> = std::iter::from_fn(|| s.pop().map(|(_, x)| x)).collect();
        assert_eq!(ordem, ["critico", "padrao-1", "padrao-2", "lote"]);
    }

    #[test]
    fn fila_cheia_devolve_o_item() {
        let q = SharedQueue::new(2);
        assert!(q.try_push(Class::P2, 1).is_ok());
        assert!(q.try_push(Class::P2, 2).is_ok());
        assert_eq!(q.try_push(Class::P0, 3), Err(3));
    }

    // ---- perguntas e probabilidades

    #[test]
    fn confianca_vai_de_zero_a_um() {
        assert!(confidence(&[0.5, 0.5]).abs() < 1e-12);
        assert!((confidence(&[1.0, 0.0, 0.0]) - 1.0).abs() < 1e-12);
        let c = confidence(&[0.9, 0.05, 0.05]);
        assert!(c > 0.5 && c < 1.0);
    }

    fn choice_q() -> Vec<Question> {
        parse_questions(&json!({
            "rota": { "type": "choice", "instructions": "Para onde vai?",
                      "criteria": { "financeiro": "cobranças", "suporte": "dúvidas" } }
        }))
        .unwrap()
    }

    #[test]
    fn resposta_do_modelo_aceita_texto_em_volta_e_normaliza() {
        let qs = choice_q();
        let t = "Claro!\n```json\n{\"answers\":{\"rota\":[3, 1]}}\n```";
        let p = parse_answers(t, &qs).unwrap();
        assert!((p[0][0] - 0.75).abs() < 1e-12 && (p[0][1] - 0.25).abs() < 1e-12);
    }

    #[test]
    fn resposta_invalida_e_rejeitada() {
        let qs = choice_q();
        assert!(parse_answers("sem json", &qs).is_none());
        assert!(parse_answers(r#"{"answers":{"rota":[1]}}"#, &qs).is_none()); // tamanho errado
        assert!(parse_answers(r#"{"answers":{"rota":[-1, 2]}}"#, &qs).is_none()); // negativo
        assert!(parse_answers(r#"{"answers":{"rota":[0, 0]}}"#, &qs).is_none()); // soma zero
        assert!(parse_answers(r#"{"answers":{}}"#, &qs).is_none()); // pergunta ausente
    }

    #[test]
    fn esquema_do_pedido_e_validado() {
        assert!(parse_questions(&json!({})).is_err());
        assert!(parse_questions(&json!({"a": {"type":"score","instructions":"x","criteria":["so um"]}})).is_err());
        assert!(parse_questions(&json!({"a": {"type":"choice","instructions":"x","criteria":{"a":"1"}}})).is_err());
        assert!(parse_questions(&json!({"a": {"type":"xyz","instructions":"x"}})).is_err());
        assert!(parse_questions(&json!({"a": {"type":"noul"}})).is_err()); // sem instructions
        assert!(parse_questions(&json!({"a": {"type":"noul","instructions":"x"}})).is_ok());
    }

    #[test]
    fn score_e_a_media_dos_niveis() {
        let qs = parse_questions(&json!({"s": {"type":"score","instructions":"x","criteria":["baixo","medio","alto"]}})).unwrap();
        let a = answer(&qs[0], &[0.0, 0.5, 0.5]);
        assert_eq!(a["score"], json!(1.5));
        assert_eq!(a["legend"]["2"], json!("alto"));
    }

    // ---- servidor de ponta a ponta, com um modelo simulado

    struct Mock {
        url: String,
        calls: Arc<AtomicUsize>,
    }

    /// Modelo simulado: devolve as respostas em ordem (a última se repete).
    fn mock_upstream(replies: Vec<&str>, delay_ms: u64) -> Mock {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut q: VecDeque<String> = replies.into_iter().map(String::from).collect();
        let c = calls.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                c.fetch_add(1, AO::SeqCst);
                std::thread::sleep(Duration::from_millis(delay_ms));
                let content = if q.len() > 1 { q.pop_front().unwrap() } else { q[0].clone() };
                let out = json!({ "choices": [{ "message": { "content": content } }],
                                  "usage": { "prompt_tokens": 100, "completion_tokens": 20 } });
                let _ = req.respond(tiny_http::Response::from_string(out.to_string()));
            }
        });
        Mock { url, calls }
    }

    fn start_jev(upstream: &str, max_ctx_tokens: u64, keys: Vec<(String, Class)>) -> String {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let cfg = Config {
            model: "teste".into(),
            upstream: upstream.into(),
            max_ctx_tokens,
            max_queue: 8,
            attempts: 3,
            keys,
        };
        std::thread::spawn(move || serve(server, cfg));
        url
    }

    fn post(url: &str, headers: &[(&str, &str)], body: Value) -> (u16, Value, HashMap<String, String>) {
        let mut r = ureq::post(&format!("{url}/v1/systemone"));
        for (k, v) in headers {
            r = r.set(k, v);
        }
        let resp = match r.send_json(body) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("falha de rede: {e}"),
        };
        let status = resp.status();
        let hs = ["x-rreia-class", "x-rreia-attempts", "x-rreia-queue-ms"]
            .iter()
            .filter_map(|h| resp.header(h).map(|v| (h.to_string(), v.to_string())))
            .collect();
        (status, resp.into_json().unwrap_or(Value::Null), hs)
    }

    fn pedido() -> Value {
        json!({
            "state": { "assunto": "Cobrança duplicada no cartão", "cliente": "prime" },
            "questions": {
                "rota": { "type": "choice", "instructions": "Qual fila atende?",
                          "criteria": { "financeiro": "cobranças", "suporte": "dúvidas técnicas" } },
                "urgencia": { "type": "score", "instructions": "Quão urgente?", "criteria": ["baixa", "media", "alta"] },
                "reembolso": { "type": "noul", "instructions": "O cliente pede reembolso?" }
            }
        })
    }

    const BOA: &str = r#"{"answers":{"rota":[0.9,0.1],"urgencia":[0.1,0.3,0.6],"reembolso":[0.8,0.2]}}"#;

    #[test]
    fn ponta_a_ponta_choice_score_e_noul() {
        let m = mock_upstream(vec![BOA], 0);
        let jev = start_jev(&m.url, 10_000, vec![]);
        let (st, body, h) = post(&jev, &[], pedido());
        assert_eq!(st, 200, "{body}");
        assert_eq!(body["answers"]["rota"]["choice"], "financeiro");
        assert_eq!(body["answers"]["rota"]["probabilities"]["financeiro"], json!(0.9));
        assert_eq!(body["answers"]["urgencia"]["score"], json!(1.5));
        assert_eq!(body["answers"]["reembolso"]["noul"], json!(0.8));
        assert_eq!(body["usage"]["input_tokens"], 100);
        assert_eq!(h["x-rreia-class"], "P2");
        assert_eq!(h["x-rreia-attempts"], "1");
    }

    #[test]
    fn repete_quando_o_modelo_erra_o_formato() {
        let m = mock_upstream(vec!["não sei dizer", BOA], 0);
        let jev = start_jev(&m.url, 10_000, vec![]);
        let (st, body, h) = post(&jev, &[], pedido());
        assert_eq!(st, 200, "{body}");
        assert_eq!(h["x-rreia-attempts"], "2");
        assert_eq!(m.calls.load(AO::SeqCst), 2);
        assert_eq!(body["usage"]["input_tokens"], 200); // as duas tentativas contam
    }

    #[test]
    fn desiste_com_502_depois_de_3_respostas_invalidas() {
        let m = mock_upstream(vec!["lixo"], 0);
        let jev = start_jev(&m.url, 10_000, vec![]);
        let (st, _, _) = post(&jev, &[], pedido());
        assert_eq!(st, 502);
        assert_eq!(m.calls.load(AO::SeqCst), 3);
    }

    #[test]
    fn modelo_fora_do_ar_da_502() {
        let jev = start_jev("http://127.0.0.1:1", 10_000, vec![]);
        assert_eq!(post(&jev, &[], pedido()).0, 502);
    }

    #[test]
    fn pedido_invalido_da_400_sem_chamar_o_modelo() {
        let m = mock_upstream(vec![BOA], 0);
        let jev = start_jev(&m.url, 10_000, vec![]);
        let ruim = json!({ "state": "x", "questions": { "a": { "type": "score", "instructions": "x", "criteria": ["um só"] } } });
        assert_eq!(post(&jev, &[], ruim).0, 400);
        assert_eq!(post(&jev, &[], json!({ "questions": {} })).0, 400);
        assert_eq!(m.calls.load(AO::SeqCst), 0);
    }

    #[test]
    fn entrada_maior_que_o_contexto_da_413_pela_l3() {
        let m = mock_upstream(vec![BOA], 0);
        let jev = start_jev(&m.url, 50, vec![]);
        let (st, body, _) = post(&jev, &[], pedido());
        assert_eq!(st, 413, "{body}");
        assert_eq!(m.calls.load(AO::SeqCst), 0);
    }

    #[test]
    fn chave_define_a_classe_maxima() {
        let m = mock_upstream(vec![BOA], 0);
        let keys = vec![("chefe".to_string(), Class::P0), ("lote".to_string(), Class::P3)];
        let jev = start_jev(&m.url, 10_000, keys);

        assert_eq!(post(&jev, &[], pedido()).0, 401);
        assert_eq!(post(&jev, &[("Authorization", "Bearer errada")], pedido()).0, 401);

        // "lote" pede P0, mas a chave só vai até P3.
        let (st, _, h) = post(&jev, &[("Authorization", "Bearer lote"), ("X-RREIA-Class", "P0")], pedido());
        assert_eq!((st, h["x-rreia-class"].as_str()), (200, "P3"));

        // "chefe" pode pedir P0.
        let (_, _, h) = post(&jev, &[("Authorization", "Bearer chefe"), ("X-RREIA-Class", "P0")], pedido());
        assert_eq!(h["x-rreia-class"], "P0");

        // Pedindo menos do que a chave permite, vale o pedido.
        let (_, _, h) = post(&jev, &[("Authorization", "Bearer chefe"), ("X-RREIA-Class", "P3")], pedido());
        assert_eq!(h["x-rreia-class"], "P3");
    }

    #[test]
    fn p0_passa_na_frente_de_p3_na_fila() {
        // O modelo leva 250 ms por chamada. Enquanto a 1ª roda, entram P3 e depois P0.
        let m = mock_upstream(vec![BOA], 250);
        let jev = start_jev(&m.url, 10_000, vec![]);
        let fim: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(vec![]));

        let envia = |nome: &'static str, classe: &'static str, espera_ms: u64| {
            let (jev, fim) = (jev.clone(), fim.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(espera_ms));
                let (st, _, _) = post(&jev, &[("X-RREIA-Class", classe)], pedido());
                assert_eq!(st, 200);
                fim.lock().unwrap().push(nome);
            })
        };
        let ts = vec![envia("primeira", "P2", 0), envia("lote", "P3", 80), envia("critica", "P0", 160)];
        for t in ts {
            t.join().unwrap();
        }
        assert_eq!(*fim.lock().unwrap(), ["primeira", "critica", "lote"]);
    }

    #[test]
    fn ready_e_rota_desconhecida() {
        let m = mock_upstream(vec![BOA], 0);
        let jev = start_jev(&m.url, 777, vec![]);
        let r: Value = ureq::get(&format!("{jev}/ready")).call().unwrap().into_json().unwrap();
        assert_eq!(r["ready"], true);
        assert_eq!(r["max_ctx_tokens"], 777);
        assert!(matches!(ureq::get(&format!("{jev}/nada")).call(), Err(ureq::Error::Status(404, _))));
    }
}
