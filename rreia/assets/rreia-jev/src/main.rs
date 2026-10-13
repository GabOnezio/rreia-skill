//! Jev Local do RREIA. Configuração por variáveis de ambiente; veja o README.

use rreia_jev::{admit, serve, Class, Config, Decision, Hardware, ModelSpec, Policy};

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_str(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// RREIA_KEYS="chave-a:P0,chave-b:P2": cada chave com a maior classe que pode pedir.
fn parse_keys(s: &str) -> Vec<(String, Class)> {
    s.split(',')
        .filter_map(|kv| {
            let (k, c) = kv.split_once(':')?;
            Some((k.trim().to_string(), Class::parse(c)?))
        })
        .collect()
}

fn main() {
    // Padrões: nível R1 (PC sem GPU, 16 GB, DDR4-3200 dual) e Llama 3.1 8B em Q4_K_M.
    let hw = Hardware {
        ram_gb: env_f64("RREIA_RAM_GB", 16.0),
        bandwidth_gbs: env_f64("RREIA_BANDWIDTH_GBS", 51.2),
        reserve_gb: env_f64("RREIA_RESERVE_GB", 2.0),
    };
    let weights = env_f64("RREIA_WEIGHTS_GB", 4.9);
    let model = ModelSpec {
        name: env_str("RREIA_MODEL", "llama-3.1-8b-q4_k_m"),
        weights_gb: weights,
        active_gb: env_f64("RREIA_ACTIVE_GB", weights),
        layers: env_f64("RREIA_LAYERS", 32.0) as u32,
        kv_heads: env_f64("RREIA_KV_HEADS", 8.0) as u32,
        head_dim: env_f64("RREIA_HEAD_DIM", 128.0) as u32,
    };
    let policy = Policy { mem_fraction: env_f64("RREIA_MEM_FRACTION", 0.75), eta: env_f64("RREIA_ETA", 0.5) };

    let max_ctx_tokens = match admit(&hw, &model, &policy, env_f64("RREIA_MIN_TOK_S", 5.0)) {
        Decision::Accept { max_ctx_tokens, tok_s_floor } => {
            println!("admitido: contexto máximo {max_ctx_tokens} tokens, piso de {tok_s_floor:.1} tok/s");
            max_ctx_tokens
        }
        Decision::RejectMemory { budget_gb } => {
            eprintln!("recusado (L1): faltam {:.1} GB de memória para este modelo", -budget_gb);
            std::process::exit(2);
        }
        Decision::RejectSpeed { tok_s_floor, required } => {
            eprintln!("recusado (L2): piso de {tok_s_floor:.1} tok/s abaixo do mínimo de {required:.1}");
            std::process::exit(2);
        }
    };

    let keys = parse_keys(&env_str("RREIA_KEYS", ""));
    let bind = env_str("RREIA_BIND", "127.0.0.1:8080");
    let server = tiny_http::Server::http(&bind).unwrap_or_else(|e| {
        eprintln!("não foi possível abrir {bind}: {e}");
        std::process::exit(1);
    });
    println!("ouvindo em http://{bind} ({} chaves)", keys.len());
    serve(
        server,
        Config {
            model: model.name,
            upstream: env_str("RREIA_UPSTREAM", "http://127.0.0.1:8000"),
            max_ctx_tokens,
            max_queue: env_f64("RREIA_MAX_QUEUE", 64.0) as usize,
            attempts: 3,
            keys,
        },
    );
}
