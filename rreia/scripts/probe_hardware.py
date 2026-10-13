#!/usr/bin/env python3
"""RREIA: sonda de hardware e verificação de compatibilidade (L1, L2, L3).

Somente leitura: não escreve arquivos, não usa a rede, não instala nada.
Só a biblioteca padrão do Python 3.8+. Funciona em Linux, macOS e Windows.

Exemplos:
  python probe_hardware.py                      # só descreve a máquina
  python probe_hardware.py --model llama3.1-8b  # descreve e verifica compatibilidade
  python probe_hardware.py --ram-gb 16 --bandwidth-gbs 51.2 --weights-gb 4.9 \
      --layers 32 --kv-heads 8 --head-dim 128    # sem sondar: usa o que o humano informou
  python probe_hardware.py --measure            # mede uma cópia de memória (estimativa grosseira)

Saída: JSON em stdout (o agente lê isso). Código de saída: 0 compatível ou só descrição,
2 recusado por memória (L1), 3 recusado por velocidade (L2).
"""
import argparse, ctypes, json, os, platform, re, shutil, subprocess, sys, time

# Modelos de referência em Q4_K_M. Pesos em GB são aproximados: confira no cartão do modelo.
# Para qualquer outro modelo, leia config.json (num_hidden_layers, num_key_value_heads,
# head_dim ou hidden_size / num_attention_heads) e passe pelas opções --layers etc.
PRESETS = {
    "llama3.1-8b": dict(weights_gb=4.9, layers=32, kv_heads=8, head_dim=128),
    "llama3.2-3b": dict(weights_gb=2.0, layers=28, kv_heads=8, head_dim=128),
    "llama3.2-1b": dict(weights_gb=0.8, layers=16, kv_heads=8, head_dim=64),
}

# Larguras de banda típicas por tipo de memória, em GB/s, só para estimar quando nada foi medido.
BANDWIDTH_HINTS = {
    "ddr4-3200-dual": 51.2,
    "ddr5-5600-dual": 89.6,
    "lpddr4x-pi5": 17.1,      # Raspberry Pi 5: LPDDR4X-4267 em barramento de 32 bits
    "jetson-orin-nano-super": 102.0,
}


def run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=10).stdout.strip()
    except Exception:
        return ""


def ram_gb():
    s = platform.system()
    try:
        if s == "Linux":
            for line in open("/proc/meminfo"):
                if line.startswith("MemTotal"):
                    return int(line.split()[1]) / 1024 / 1024
        if s == "Darwin":
            return int(run(["sysctl", "-n", "hw.memsize"])) / 1e9
        if s == "Windows":
            class MS(ctypes.Structure):
                _fields_ = [("l", ctypes.c_ulong), ("m", ctypes.c_ulong)] + [
                    (n, ctypes.c_ulonglong) for n in ("tp", "ap", "tpf", "apf", "tv", "av", "ae")]
            m = MS(); m.l = ctypes.sizeof(MS)
            ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(m))
            return m.tp / 1e9
    except Exception:
        pass
    return None


def cpu_name():
    s = platform.system()
    if s == "Linux":
        try:
            for line in open("/proc/cpuinfo"):
                if line.lower().startswith(("model name", "hardware")):
                    return line.split(":", 1)[1].strip()
        except Exception:
            pass
    if s == "Darwin":
        return run(["sysctl", "-n", "machdep.cpu.brand_string"]) or platform.processor()
    return platform.processor() or platform.machine()


def board_model():
    try:
        return open("/proc/device-tree/model", "rb").read().decode(errors="ignore").strip("\x00 \n")
    except Exception:
        return ""


def nvidia_gpus():
    if not shutil.which("nvidia-smi"):
        return []
    out = run(["nvidia-smi", "--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
    gpus = []
    for line in out.splitlines():
        name, _, mem = line.partition(",")
        try:
            gpus.append({"name": name.strip(), "vram_gb": round(float(mem) / 1024, 1)})
        except ValueError:
            pass
    return gpus


def measure_bandwidth_gbs():
    """Cópia de 256 MB. Subestima a banda real (um núcleo, Python). Serve só como piso grosseiro."""
    n = 256 * 1024 * 1024
    src = bytearray(n)
    best = 0.0
    for _ in range(3):
        t = time.perf_counter()
        dst = bytes(src)  # lê n e escreve n
        dt = time.perf_counter() - t
        best = max(best, 2 * n / dt / 1e9)
        del dst
    return round(best, 1)


def tier(ram, board, cpu, gpus, arch):
    b, c = board.lower(), cpu.lower()
    if "raspberry pi" in b:
        return "R0"
    if "jetson" in b or "orin" in b:
        return "R2"
    if platform.system() == "Darwin" and arch == "arm64":
        return "R3" if (ram or 0) >= 32 else "R1"
    if any(g["vram_gb"] >= 16 for g in gpus):
        return "R3"
    if (ram or 0) >= 16:
        return "R1"
    return "R0"


def admit(ram, bw, weights, active, layers, kv, hd, reserve, fraction, eta, min_tok_s):
    budget = fraction * ram - weights - reserve
    if budget <= 0:
        return {"decision": "reject_memory", "law": "L1", "budget_gb": round(budget, 2),
                "missing_gb": round(-budget, 2)}
    floor = eta * bw / active
    if floor < min_tok_s:
        return {"decision": "reject_speed", "law": "L2", "tok_s_floor": round(floor, 2),
                "required": min_tok_s}
    kv_bytes = 2 * layers * kv * hd * 2
    return {"decision": "accept", "budget_gb": round(budget, 2), "tok_s_floor": round(floor, 2),
            "tok_s_ceiling": round(bw / active, 2), "kv_bytes_per_token": kv_bytes,
            "max_ctx_tokens": int(budget * 1e9 / kv_bytes)}


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    a.add_argument("--model", choices=sorted(PRESETS), help="modelo de referência")
    a.add_argument("--weights-gb", type=float); a.add_argument("--active-gb", type=float)
    a.add_argument("--layers", type=int); a.add_argument("--kv-heads", type=int)
    a.add_argument("--head-dim", type=int)
    a.add_argument("--ram-gb", type=float, help="RAM (ou memória da GPU/unificada) informada pelo humano")
    a.add_argument("--bandwidth-gbs", type=float, help="largura de banda de memória em GB/s")
    a.add_argument("--memory-kind", choices=sorted(BANDWIDTH_HINTS), help="usa a banda típica desse tipo")
    a.add_argument("--reserve-gb", type=float, default=2.0)
    a.add_argument("--fraction", type=float, default=0.75, help="f da L1")
    a.add_argument("--eta", type=float, default=0.5, help="η da L2")
    a.add_argument("--min-tok-s", type=float, default=5.0)
    a.add_argument("--measure", action="store_true", help="mede uma cópia de memória (aloca 512 MB por instantes)")
    o = a.parse_args()

    arch = platform.machine().lower()
    info = {"os": f"{platform.system()} {platform.release()}", "arch": arch,
            "cpu": cpu_name(), "cpu_threads": os.cpu_count(), "board": board_model(),
            "ram_gb": round(ram_gb() or 0, 1) or None, "gpus": nvidia_gpus()}
    info["tier_guess"] = tier(info["ram_gb"], info["board"], info["cpu"], info["gpus"], arch)
    if o.measure:
        info["measured_copy_gbs_lower_bound"] = measure_bandwidth_gbs()

    ram = o.ram_gb or info["ram_gb"]
    bw = o.bandwidth_gbs or BANDWIDTH_HINTS.get(o.memory_kind or "")
    out = {"probe": info}

    p = dict(PRESETS.get(o.model, {}))
    for k, v in (("weights_gb", o.weights_gb), ("layers", o.layers), ("kv_heads", o.kv_heads), ("head_dim", o.head_dim)):
        if v is not None:
            p[k] = v
    missing = [k for k in ("weights_gb", "layers", "kv_heads", "head_dim") if k not in p]
    if not o.model and len(missing) == 4:
        print(json.dumps(out, ensure_ascii=False, indent=2)); return 0
    if missing or not ram or not bw:
        out["check"] = {"decision": "need_more_info",
                        "missing": missing + (["ram_gb"] if not ram else []) + (["bandwidth_gbs"] if not bw else [])}
        print(json.dumps(out, ensure_ascii=False, indent=2)); return 0

    out["check"] = admit(ram, bw, p["weights_gb"], o.active_gb or p["weights_gb"], p["layers"],
                         p["kv_heads"], p["head_dim"], o.reserve_gb, o.fraction, o.eta, o.min_tok_s)
    out["check"]["inputs"] = {"ram_gb": ram, "bandwidth_gbs": bw, **p}
    print(json.dumps(out, ensure_ascii=False, indent=2))
    return {"accept": 0, "reject_memory": 2, "reject_speed": 3}[out["check"]["decision"]]


if __name__ == "__main__":
    sys.exit(main())
