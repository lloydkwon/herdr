# herdr


<p align="center">
  <img src="assets/logo.png" alt="herdr" width="100" />
</p>

<p align="center">
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/herdrdev/herdr/releases"><img src="https://img.shields.io/github/downloads/herdrdev/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/herdrdev/herdr/stargazers"><img src="https://img.shields.io/github/stars/herdrdev/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/herdrdev/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/herdrdev/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **always running** — herdr is a background server; the terminals live inside it. close the lid, drop the network, or restart the machine; agents keep working and sessions come back. reattach from any terminal, or over ssh.
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so.
- **agent-native** — agents drive herdr through the cli and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. herdr doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows beta: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [binaries](https://github.com/herdrdev/herdr/releases)

> **이 저장소는 [herdrdev/herdr](https://github.com/herdrdev/herdr)의 fork입니다.**
> 위 설치 방법(`install.sh`, Homebrew, mise, Releases)은 전부 **upstream 공식 릴리즈**를 설치합니다.
> 이 fork에는 릴리즈 아티팩트가 없으므로, fork의 변경사항을 쓰려면 [fork에서 빌드해서 설치](#fork에서-빌드해서-설치)를 따르세요.

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@herdr.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## fork에서 빌드해서 설치

이 fork에는 릴리즈 바이너리가 없습니다. 소스에서 직접 빌드해야 합니다.

### 선행 조건

| 도구 | 버전 | 비고 |
| --- | --- | --- |
| Rust | 1.96.1 | `rust-toolchain.toml`이 버전을 고정하므로 rustup만 있으면 자동으로 맞춰집니다 |
| Zig | 0.15.2 | `build.rs`가 벤더링된 `vendor/libghostty-vt`를 `zig build`로 컴파일합니다. `PATH`의 `zig`를 쓰거나 `ZIG=<path>`로 지정 |
| just, cargo-nextest, python3, bun | 선택 | 테스트 레시피(`just test`, `just check`)를 돌릴 때만 필요 |

Zig가 PATH에 없거나 버전이 다르면 이렇게 지정합니다:

```bash
ZIG=/path/to/zig-0.15.2/zig cargo build --release
```

### 클론과 빌드

```bash
git clone https://github.com/lloydkwon/herdr
cd herdr
git remote add upstream https://github.com/herdrdev/herdr.git   # upstream 추적용(선택)

cargo build --release      # target/release/herdr
```

개발 중에는 debug 빌드가 훨씬 빠릅니다:

```bash
cargo build --bin herdr    # target/debug/herdr
```

### 설치

공식 설치본(`install.sh`로 깐 `~/.local/bin/herdr`)을 유지하면서 fork 빌드를 함께 두려면, 다른 이름으로 복사합니다.
서버 데몬은 자기 자신의 실행 파일 경로(`current_exe`)로 뜨기 때문에 이름을 바꿔도 정상 동작합니다.

```bash
install -m 755 target/release/herdr ~/.local/bin/herdr-dev
herdr-dev --version
```

공식 설치본을 fork 빌드로 덮어쓰려면:

```bash
install -m 755 target/release/herdr ~/.local/bin/herdr
```

> 두 빌드를 같이 쓸 때는 fork 빌드를 이름 있는 세션(`--session <name>`)으로 분리하세요.
> 같은 세션을 공유한 상태에서 두 빌드의 `PROTOCOL_VERSION`이 달라지면 `protocol_mismatch`로 명령이 거부됩니다.

### 실행

```bash
herdr-dev --session dev            # 설치한 fork 빌드
```

빌드 디렉터리에서 바로 돌릴 때는, 상위 herdr 세션에서 상속된 소켓 오버라이드를 지우고 실행합니다:

```bash
env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH \
  ./target/debug/herdr --session dev
```

세션 정리는 `herdr server stop`(같은 `HERDR_SESSION` 지정) 또는 `prefix+q`로 detach.

### 테스트

```bash
just test        # 유닛 테스트
just check       # 포매팅 + 테스트 + 유지보수 스크립트 검사
```

`just`가 없다면 개별 명령으로도 돌릴 수 있습니다:

```bash
cargo nextest run --locked      # 또는 cargo test
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```

### upstream 따라가기

```bash
git fetch upstream
git rebase upstream/master      # 또는 git merge upstream/master
```

upstream(`herdrdev/herdr`)은 초대받지 않은 구현 PR을 자동으로 닫습니다. 자세한 규칙은 [`CONTRIBUTING.md`](./CONTRIBUTING.md)를 참고하세요.

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
