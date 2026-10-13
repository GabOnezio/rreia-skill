# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versionamento [SemVer](https://semver.org/lang/pt-BR/).

Enquanto a versão for 0.x, a skill está em desenvolvimento inicial: qualquer coisa pode mudar de uma versão menor para outra, e ela ainda não é segura para uso em produção.

## [0.1.0] - 2026-10-13

Primeira versão pública.

### Adicionado
- Skill `rreia` (`rreia/SKILL.md`): fluxo em seis passos, de "onde o sistema vai rodar" até "verificar e relatar".
- Referências: leis L1 a L9 com fórmulas e exemplo resolvido, níveis de hardware R0 a R3, Jev Local, política de custo e fallback, modelo de `RREIA.md`, instalação.
- `rreia/scripts/probe_hardware.py`: sonda de hardware somente leitura e verificação L1 a L3.
- `rreia/assets/rreia-jev` 0.2.0: Jev Local em Rust, com 20 testes sobre um modelo simulado.
- `README.md`, `LICENSE` (MIT) e este changelog.

### Corrigido
- `rreia/references/laws.md` dizia que L1 a L7 estavam implementadas e testadas no `rreia-jev`. O código implementa L1 a L4 e L6; o texto agora diz isso.

### Alterado
- `metadata.version` do `SKILL.md` passou de `"0.1"` para `"0.1.0"`, para seguir o SemVer.

[0.1.0]: https://github.com/GabOnezio/rreia-skill/releases/tag/v0.1.0
