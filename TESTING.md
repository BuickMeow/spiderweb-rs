# 测试指南（TESTING.md）

发帖 / 合代码之前照着跑一遍。分三层：**自动化测试 → 手工功能清单 → 性能测量**。

## 0. 快速自检（每条改动后 / 发帖前）

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p spiderweb-app
```

## 1. 自动化测试覆盖了什么

| 层 | 内容 | 位置 |
|---|---|---|
| core 差分测试 | 8 组对照向量，2000+ 例，全部由 Python 原版生成 | `crates/spiderweb-core/tests/*_vectors.rs` + `tools/gen_*_vectors.py` |
| Domino 编解码 | 与 Python 互操作（编码解压负载逐字节一致 / 解码 Python 数据） | `crates/spiderweb-domino/tests/` |
| MIDI 导出 | 与 Python 输出**逐字节一致**（手写 SMF 头校验，不依赖 MIDI 库） | `crates/spiderweb-io/tests/midi_vectors.rs` |
| app 单测 | 输入状态机、curve 把手、scrub、i18n 键完整性等 | `crates/spiderweb-app/src/**` 的 `#[cfg(test)]` |

**回归基线**（当前）：workspace 全绿；core 微基准见第 3.4 节。

注意：差分测试的 Python 源默认是 `~/Documents/GitHub/Spiderweb-main`（1.1.0 参考副本）；
跟进 1.2.0 时用 `SPIDERWEB_SRC=/Users/jieneng/Documents/GitHub/Spiderweb-1.2.0/scripts python3 tools/gen_*.py`。

## 2. 手工功能清单（GUI，无法自动化的部分）

每次大改后过一遍；发帖时在 README/帖子写明"交互手工测试通过"。

- [ ] 工具逐个：Line / Polyline / Freehand / Curve / Arc / Square / Circle / Triangle / Custom / Funnel / Text
      （画出来、拖点、删点、撤销/重做、重新选中）
- [ ] Select：点选 / Ctrl 多选 / 拖动 / 把手 / Delete / Duplicate / Ctrl+A
- [ ] 右键菜单（形状菜单、曲线/漏斗高亮项）、双击右键切工具
- [ ] 播放（Space）、右拖试听、播放线翻页
- [ ] 导出 MIDI（在 DAW 或 Domino 里打开检查）
- [ ] Domino 复制 / 粘贴（**仅 Windows**，macOS 上按钮应给出提示而不是崩溃）
- [ ] 打开 1.1.0 / 1.2.0 的 `autosave.json`、另存再打开、autosave 备份恢复
- [ ] Drawer：画形状、存库（`shapes/*.json`）、Use、在卷帘里放出来
- [ ] 力度面板：Linear / Curve / Pencil、Enter 确认
- [ ] 帮助 F1（可搜索）、首次提示、错误日志 `errors.log`
- [ ] 窗口缩放、Retina（ppp≠1）、侧栏滚动、多显示器拖动
- [ ] 大工程冒烟：>= 100 万音符时拖动/播放/撤销不崩（见第 3 节）

## 3. 性能测量

统一要求：**release 构建**、记录机器型号 / macOS 版本 / 分辨率 / 是否 Retina；
帧时间记 p50 / p95 / max，不要只写平均。

### 3.1 生成基准工程

```bash
python3 tools/gen_bench_project.py --notes 1000000 -o bench.json    # 100 万
python3 tools/gen_bench_project.py --notes 5000000 -o bench-big.json # 500 万
```

把生成的 JSON 拷到可执行文件旁边改名 `autosave.json` 启动，或在应用里用 Open… 打开。
文件兼容 Python 原版工程格式（Python 也能读）。

### 3.2 开 perf HUD

```bash
SPIDERWEB_PERF=1 cargo run --release -p spiderweb-app
```

- 右上角 HUD：帧时间 EMA、形状数、音符数
- stderr 打印：`[perf] load+render … ms`、每次重算 `[perf] shapes_changed … ms`

### 3.3 记录表（同屏性能）

在 500 万工程上分别测量，填进 README 或帖子：

| 场景 | p50 | p95 | max | 备注 |
|---|---|---|---|---|
| 静置（画面不动） | | | | |
| 平移 / 缩放 | | | | wgpu 路径只更新 uniform，不应重传 instance |
| 拖动形状 | | | | 每次改动全量重传 instance，看最坏帧 |
| 播放 | | | | |
| 加载工程（到首帧） | | | | `[perf] load+render` |
| 内存 RSS | | | | `ps -o rss= -p $(pgrep -f spiderweb-app)` |

### 3.4 core 微基准（可在 CI 里看趋势）

```bash
cargo bench -p spiderweb-core --bench engine
```

当前参考值（Apple M 系列，release，仅供对比趋势）：

| 用例 | 时间 |
|---|---|
| line_notes_64keys | ~6 µs |
| tumour_line_64keys | ~0.66 ms |
| custom_spam_100k | ~1.8 ms |
| funnel_spam_100k | ~4.7 ms |
| render_single_100k | ~2.5 ms |

## 4. CI 与"让所有人看到性能"

已经配好两个工作流：

- **`.github/workflows/ci.yml`**：push / PR 时在 Ubuntu + macOS + Windows 上跑
  `fmt / clippy -D warnings / test / release build`。README 里贴徽章即可。
- **`.github/workflows/bench.yml`**：main 上跑 core 微基准，用
  [benchmark-action](https://github.com/benchmark-action/github-action-benchmark)
  把每个提交的数据点写进 `gh-pages` 分支，并在 PR 上自动评论性能回归（阈值 130%）。

启用性能看板（一次性，仓库 Settings 里点两下）：

1. Settings → Pages → Source 选 **Deploy from a branch** → 分支 `gh-pages`、目录 `/ (root)`；
   第一次 bench 工作流运行后该分支会自动创建。
2. 看板地址：`https://<用户名>.github.io/<仓库名>/dev/bench/`
   （本仓库为 `https://buickmeow.github.io/spiderweb-rs/dev/bench/`）。

这是"最方便让所有人看到"的方案：**一个 URL，随时间变化的折线图，按提交可追溯**；
PR 里还会自动贴回归评论。可选升级：接 [CodSpeed](https://codspeed.io)（对开源免费，
PR 里给火焰图），或把 `bench.yml` 的 job summary 链接贴到 README。

### 发帖模板

```
Tested on <机器>, <系统>, <分辨率>:
- 8 differential suites, 2000+ cases vs the original Python engine
- MIDI export byte-identical to the original; Domino codec cross-checked
- 87 unit tests, clippy -D warnings clean, release build
- Perf dashboard: https://buickmeow.github.io/spiderweb-rs/dev/bench/
- 5M notes: load <x>s, idle <x>fps, pan <x>fps, drag <x>fps, RSS <x>MB
- Known gaps: interactive UX tests in progress; Domino path is Windows-only
```
