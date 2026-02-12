# Hardware Requirements

## Development Machine

This is what you develop ON. Can be your laptop/desktop or a cloud VM.

### Minimum (Can build and test in QEMU)
| Component | Spec |
|---|---|
| CPU | 8 cores (x86_64), AVX2 support |
| RAM | 32 GB |
| Storage | 256 GB SSD (NVMe preferred) |
| GPU | Not required for dev (CPU inference is fine) |
| Network | Broadband internet (for API calls + package downloads) |
| OS | Linux (Ubuntu 22.04+ or Fedora 38+) or macOS 13+ |

### Recommended (Faster builds, local model testing)
| Component | Spec |
|---|---|
| CPU | 16+ cores (AMD Ryzen 9 / Intel i9 / Apple M2 Pro+) |
| RAM | 64 GB |
| Storage | 1 TB NVMe SSD |
| GPU | NVIDIA RTX 3090/4090 (24GB VRAM) for local model testing |
| Network | Gigabit ethernet |

---

## Target Machine (Where aiOS Actually Runs)

### Tier 1: Minimal Edge Device
For lightweight deployments with mostly API-based intelligence.

| Component | Spec |
|---|---|
| CPU | 4 cores ARM64 or x86_64 |
| RAM | 8 GB |
| Storage | 64 GB eMMC/SSD |
| GPU | None |
| Network | WiFi + Ethernet |
| AI Capability | Reactive + operational layer only (tiny models <1B) |
| API Dependency | High — needs constant internet for reasoning |
| Example Hardware | Raspberry Pi 5 8GB, Intel NUC |

### Tier 2: Standard Server (Recommended Starting Target)
The sweet spot. Can run meaningful local models while using APIs for complex tasks.

| Component | Spec |
|---|---|
| CPU | 8-16 cores x86_64 (AMD EPYC / Intel Xeon) |
| RAM | 32-64 GB DDR5 |
| Storage | 512 GB NVMe SSD |
| GPU | NVIDIA RTX 4090 (24GB) or A4000 (16GB) |
| Network | Gigabit ethernet |
| AI Capability | All four layers operational |
| API Dependency | Medium — local handles 70% of decisions |
| Example Hardware | Custom build, Dell PowerEdge, HP ProLiant |

### Tier 3: Full Autonomous Server
Can run large local models. Minimal API dependency.

| Component | Spec |
|---|---|
| CPU | 32-64 cores (AMD EPYC 9004 / Intel Xeon W) |
| RAM | 128-256 GB DDR5 ECC |
| Storage | 2 TB NVMe SSD (system) + 4 TB NVMe (models/data) |
| GPU | 2x NVIDIA A100 80GB or H100 80GB |
| Network | 10GbE |
| AI Capability | Can run 70B models locally, near-zero API dependency |
| API Dependency | Low — API only for frontier reasoning tasks |
| Example Hardware | Supermicro GPU server, Lambda Labs, custom |

---

## Cloud Development Options

If you don't have local hardware, use cloud VMs for development and testing.

### For Development + QEMU Testing
| Provider | Instance | Specs | ~Cost/hr |
|---|---|---|---|
| AWS | c6i.4xlarge | 16 vCPU, 32GB RAM, no GPU | ~$0.68 |
| GCP | n2-standard-16 | 16 vCPU, 64GB RAM, no GPU | ~$0.78 |
| Hetzner | CPX51 | 16 vCPU, 32GB RAM, no GPU | ~$0.06 |

### For GPU Testing (Local Models)
| Provider | Instance | Specs | ~Cost/hr |
|---|---|---|---|
| AWS | g5.4xlarge | 16 vCPU, 64GB RAM, A10G 24GB | ~$1.62 |
| Lambda Labs | gpu_1x_a100 | 30 vCPU, 200GB RAM, A100 80GB | ~$1.10 |
| Vast.ai | RTX 4090 | Varies | ~$0.30-0.50 |
| RunPod | RTX 4090 | 16 vCPU, 64GB, RTX 4090 24GB | ~$0.39 |

---

## Storage Breakdown (Target System)

```
/                    20 GB   Root filesystem, OS, system binaries
/var                 50 GB   Logs, runtime data, databases
/var/lib/ai/models  100 GB+  Local AI models (GGUF files)
/var/lib/ai/memory   50 GB   Vector DB, knowledge base, SQLite
/var/lib/ai/cache    20 GB   Model inference cache, tool result cache
/home                50 GB   User/task workspaces
/tmp                 10 GB   Temporary files (tmpfs recommended)
```

Minimum total: ~300 GB for Tier 2 deployment.

---

## Network Requirements

| Need | Requirement |
|---|---|
| API Calls (Claude/GPT) | Stable internet, <100ms latency preferred |
| Package Downloads | HTTP/HTTPS outbound |
| Model Downloads | HTTP/HTTPS outbound (models are 1-40GB each) |
| Management Console | SSH or HTTPS inbound on management port |
| Inter-agent Communication | localhost only (Unix domain sockets preferred) |

### Offline Operation
aiOS MUST be able to operate offline with degraded capability:
- All operational + tactical layer models run locally
- Cached API responses can be replayed for common patterns
- Only strategic layer (frontier API) is unavailable offline
- System should detect offline state and adjust decision routing

---

## GPU Compatibility

### Supported (Tested)
| GPU | VRAM | Max Local Model | Framework |
|---|---|---|---|
| NVIDIA RTX 3090 | 24 GB | 13B Q8 / 30B Q4 | CUDA + llama.cpp |
| NVIDIA RTX 4090 | 24 GB | 13B Q8 / 30B Q4 | CUDA + llama.cpp |
| NVIDIA A100 | 80 GB | 70B Q8 | CUDA + llama.cpp |
| NVIDIA H100 | 80 GB | 70B Q8 | CUDA + llama.cpp |
| AMD RX 7900 XTX | 24 GB | 13B Q8 / 30B Q4 | ROCm + llama.cpp |

### Experimental
| GPU | VRAM | Notes |
|---|---|---|
| Apple M2 Pro+ | Unified 16-96GB | Metal backend, good for dev |
| Intel Arc A770 | 16 GB | SYCL backend, limited |

### CPU-Only Inference
Viable for models up to 7B with Q4 quantization. Expect 5-15 tokens/sec on modern CPUs. Sufficient for operational layer tasks.

---

## Recommended Dev Setup for This Project

If starting from scratch, this is the most cost-effective dev setup:

1. **Development machine**: Any modern laptop/desktop with 16GB+ RAM running Linux or macOS
2. **Test VM**: Hetzner CPX51 ($0.06/hr) for QEMU-based system testing
3. **GPU testing**: RunPod RTX 4090 ($0.39/hr) on-demand for local model testing
4. **Claude API key**: For strategic layer development and Claude Code usage
5. **Total cost**: ~$50-100/month for active development
