# rreia-jev

Jev Local do RREIA: servidor de decisões tipadas (`choice`, `score`, `noul`) sobre qualquer modelo local com API compatível com a OpenAI, com admissão por hardware (L1 a L3), fila de prioridade e permissões por chave (L4) e rastro de auditoria (L6).

Uso, formato do pedido e da resposta e variáveis de ambiente: veja `../../references/jev-local.md`.

```bash
cargo test              # 20 testes, com um modelo simulado
cargo run --release     # lê RREIA_* do ambiente
```

Não foi testado contra um modelo real. As probabilidades são geradas pelo modelo como texto; verifique a calibração nos seus dados. Licença MIT.
