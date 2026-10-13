# RREIA — Recursos Responsáveis para IA de Borda

> [!WARNING]
> **Versão 0.1.0, em desenvolvimento inicial. Ainda não é segura para uso em produção.**
> Os limiares são valores de partida, as velocidades são estimativas e nada foi testado contra um modelo real. Tudo pode mudar entre versões 0.x. Veja [Estado e limitações](#estado-e-limitações).

RREIA (*Resource-Responsible Edge AI*) é uma Agent Skill: uma pasta com um `SKILL.md` que agentes de programação como o Claude Code carregam sob demanda. Ela faz o agente tratar hardware, modelo, custo, dados e deveres legais como recursos com orçamento, dono e prioridade, e resolver isso **antes de escrever código**.

A ideia de fundo: a maior parte das reclamações de "a IA é ruim" é problema de recurso. O modelo não cabe na memória, a memória é lenta para o modelo, o contexto foi cortado em silêncio, o tempo limite é menor que a geração.

*English summary at the [end](#english-summary).*

## O que a skill faz

Quando você pede um sistema que roda ou chama modelos de IA, o agente segue seis passos:

1. **Onde vai rodar.** Pergunta se é na máquina atual. Se for, pede permissão para rodar uma sonda de hardware somente leitura; se não for, entrevista você sobre o dispositivo.
2. **Modelo e custo.** O padrão é modelo local gratuito. API paga só por decisão sua, com teto de gasto e resposta sobre se os dados podem sair do dispositivo.
3. **Compatibilidade, com números.** Confere se o modelo cabe na memória (L1), se a memória é rápida o bastante (L2) e quanto contexto sobra (L3). Se não couber, não escreve o programa como se coubesse.
4. **Ações, solicitantes, permissões, leis e atividades.** Registra em um `RREIA.md` na raiz do seu projeto o que o sistema pode fazer, quem pode pedir, o que exige confirmação humana e o que fica no rastro.
5. **Construção.** Modelo atrás de um servidor compatível com a API da OpenAI; decisões tipadas (`choice`, `score`, `noul`) em vez de texto livre.
6. **Verificação e relatório.** Testes para as próprias leis e um relatório do que foi estimado, medido e não testado.

## As leis

| Lei | Regra | Onde está hoje |
|---|---|---|
| L1 Memória | Pesos + cache + reserva abaixo de uma fração da memória física | Sonda e `rreia-jev` |
| L2 Banda | `η × banda ÷ GB lidos por token` alcança o mínimo da tarefa | Sonda e `rreia-jev` |
| L3 Contexto | Contexto limitado ao que a memória comporta | Sonda e `rreia-jev` |
| L4 Prioridade | Fila P0 a P3; classe efetiva = menor entre a pedida e a da chave | `rreia-jev` |
| L5 Degradação | Reduzir antes de recusar | Só descrita, sem código |
| L6 Rastro | Um registro por decisão, sem conteúdo privado | `rreia-jev` (uma linha JSON na saída padrão) |
| L7 Causa | Classificar a falha por causa antes de trocar o modelo | Só códigos de erro distintos |
| L8 Local primeiro | Modelo local gratuito quando L1 a L3 aceitam | Política descrita, sem código |
| L9 Orçamento | Chamadas remotas param no teto de gasto | Política descrita, sem código |

Fórmulas e exemplo resolvido em [`rreia/references/laws.md`](rreia/references/laws.md).

## O que tem no repositório

```
rreia/                      a skill (é esta pasta que se instala)
  SKILL.md                  instruções que o agente segue
  references/               leis, níveis de hardware, Jev Local, custo, modelo de RREIA.md, instalação
  scripts/probe_hardware.py sonda de hardware e verificação L1 a L3 (só biblioteca padrão do Python)
  assets/rreia-jev/         Jev Local: servidor Rust de decisões tipadas, com 20 testes
README.md  CHANGELOG.md  LICENSE
```

## Instalar

```bash
git clone https://github.com/GabOnezio/rreia-skill.git

# Claude Code e Claude CLI, para todos os seus projetos:
mkdir -p ~/.claude/skills && cp -r rreia-skill/rreia ~/.claude/skills/

# ou para um projeto só:
mkdir -p <projeto>/.claude/skills && cp -r rreia-skill/rreia <projeto>/.claude/skills/
```

Depois, digite `/rreia` ou só descreva o trabalho (por exemplo, "quero um assistente offline em um Raspberry Pi 5"). Para atualizar, `git pull` e rode o mesmo `cp` de novo.

Outros agentes: veja [`rreia/references/install.md`](rreia/references/install.md). O caminho indicado para o OpenAI Codex veio de descrição de terceiros e **não foi confirmado** na documentação oficial.

## Sonda de hardware

Somente leitura: não escreve arquivos, não usa a rede, não instala nada. Precisa de Python 3.8 ou mais novo.

```bash
cd rreia/scripts

python probe_hardware.py                       # só descreve a máquina
python probe_hardware.py --model llama3.1-8b   # descreve e verifica compatibilidade
python probe_hardware.py --model llama3.1-8b --ram-gb 16 --memory-kind ddr4-3200-dual
                                               # faz a conta com os valores que você informar
                                               # (a descrição da máquina local continua na saída)
```

A saída é JSON. Código de saída: 0 compatível, 2 recusado por memória (L1), 3 recusado por velocidade (L2).

Exemplo: PC de 16 GB com DDR4-3200 em dois canais e Llama 3.1 8B em Q4_K_M. Sobram 5,1 GB para o contexto, o que dá 38.909 tokens, com piso calculado de 5,22 tok/s. O mesmo modelo em 8 GB é recusado pela L1. São contas, não medições.

## Jev Local (`rreia-jev`)

Servidor em Rust que recebe o estado da aplicação e perguntas tipadas e devolve, para cada uma, uma opção com probabilidade e confiança. Roda sobre qualquer modelo local com API compatível com a OpenAI (`llama-server`, Ollama, mistral.rs). Acrescenta admissão por hardware, fila de prioridade com permissão por chave e rastro de auditoria.

```bash
cd rreia/assets/rreia-jev
cargo test              # 20 testes, com um modelo simulado
cargo run --release     # lê as variáveis RREIA_* do ambiente
```

Formato do pedido, da resposta e variáveis em [`rreia/references/jev-local.md`](rreia/references/jev-local.md). O crate tem versão própria (0.2.0), separada da versão da skill.

## Estado e limitações

O que você precisa saber antes de confiar em qualquer número ou em qualquer parte do código:

- **Leis e limiares são proposta do autor, não resultado experimental.** `f = 0,75` e `η = 0,5` são valores de partida, a calibrar com medição na máquina real.
- **Velocidades são ordens de grandeza** tiradas de fontes publicadas e de fórmula, não medições.
- **O `rreia-jev` nunca rodou contra um modelo real.** Os 20 testes usam um modelo simulado.
- **Probabilidade e confiança são escritas pelo modelo como texto**, não lidas dos logits. A calibração não foi verificada.
- **Só L1 a L4 e L6 têm código.** L5, L7, L8 e L9 estão descritas, e cabe à sua aplicação implementá-las.
- **O servidor `rreia-jev` não é para rede aberta.** É HTTP sem TLS, fica sem autenticação quando nenhuma chave é configurada, cria uma thread por requisição e grava o rastro só na saída padrão. O padrão é escutar em `127.0.0.1`; mantenha assim.
- **"Jev" é produto de terceiros** (TypeSafe AI, segundo as fontes citadas na skill). O Jev Local daqui é uma implementação independente, escrita a partir de descrições de terceiros, sem vínculo com a empresa e sem compatibilidade garantida.
- **O que a skill diz sobre LGPD e sobre o AI Act europeu é orientação de projeto**, não aconselhamento jurídico.

## Versionamento

[SemVer](https://semver.org/lang/pt-BR/). Enquanto a versão for 0.x, qualquer coisa pode mudar de uma versão menor para outra, inclusive as leis, os limiares e o formato do `RREIA.md`. Mudanças em [`CHANGELOG.md`](CHANGELOG.md).

## Licença e autoria

[MIT](LICENSE). Conceito RREIA e skill: Gabriel Onezio Ferreira.

## English summary

RREIA (Resource-Responsible Edge AI) is an Agent Skill for coding agents such as Claude Code. Before any code is written, it makes the agent check the target hardware against a model's needs (memory, memory bandwidth, context), prefer free local models and ask before using paid APIs, and write down the system's actions, principals, permissions, laws and audit trail in a `RREIA.md` file. It ships a read-only hardware probe (Python, standard library only) and Jev Local, a small Rust server for typed decisions (`choice`, `score`, `noul`) with hardware admission, a priority queue and an audit line per decision.

**Status: 0.1.0, early development, not safe for production use.** Thresholds are the author's starting values, speeds are estimates, and the server has only been tested against a simulated model. The skill's own instructions are in English: start at [`rreia/SKILL.md`](rreia/SKILL.md). To install, copy the `rreia/` folder into `~/.claude/skills/`. MIT license.
