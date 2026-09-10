# 포크 운영 안내 (lloydkwon/herdr)

이 저장소는 [herdrdev/herdr](https://github.com/herdrdev/herdr) 의 개인 포크입니다. 포크 기능은 로컬 빌드로만 쓰고
upstream 에 PR·이슈를 내지 않습니다. 이 문서는 «upstream 이 갱신돼도 포크 작업을 잃지 않고 다시 적용하는 절차»를 다룹니다.

## 핵심 규칙 세 가지

1. **포크 커밋은 항상 `origin/master` 위의 짧은 선형 열**로 유지합니다(머지 커밋 금지). 그래야 새 upstream 이 와도
   `git rebase` 한 번으로 재적용됩니다.
2. 업데이트는 **`just fork-sync` → `just fork-install`** 로만 합니다. 아래 «절대 실행하지 말 것»의 명령은 포크 빌드를
   upstream 릴리스로 덮어쓰거나 세션을 통째로 죽입니다.
3. 핸드오프 뒤에는 **`herdr` 만 다시 실행**합니다. `herdr server stop` 을 치면 pane 프로세스가 전부 종료되고 세션이
   콜드 복원되어 에이전트가 재시작됩니다(경과시간·훅 권한 초기화).

## 절대 실행하지 말 것

| 명령 | 이유 |
| --- | --- |
| `herdr update` | `current_exe()` 경로에 upstream 릴리스를 `rename` 으로 덮어씁니다. 백업을 남기지 않습니다. |
| `herdr channel set …` | 설정을 바꾼 직후 `self_update` 를 즉시 호출합니다. |
| `install.sh` 재실행 | `~/.local/bin/herdr` 를 `mv` 로 덮어씁니다. |
| 핸드오프 뒤 `herdr server stop` / `herdr session stop` | 모든 pane 을 kill 하고 콜드 복원합니다. |

백그라운드 버전 확인(30분 주기)은 알림만 냅니다. 상태바의 «update ready» 칩이 유혹이 되므로
`~/.config/herdr/config.toml` 에서 꺼 둡니다(설정은 hot-reload 됩니다):

```toml
[update]
version_check = false
```

`Cargo.toml` 의 버전은 upstream 값 그대로 둡니다. 업데이터는 `major.minor.patch` 를 엄격히 숫자 비교하므로
`0.9.0-fork.1` 같은 접미사는 파싱 실패로 **panic** 합니다. `HERDR_BUILD_CHANNEL` 도 손대지 마세요.

## 선행 도구

| 도구 | 버전 | 비고 |
| --- | --- | --- |
| Rust | `rust-toolchain.toml` 이 고정 | rustup 만 있으면 자동으로 맞춰집니다 |
| Zig | 0.15.2 | `build.rs` 가 벤더링된 `vendor/libghostty-vt` 를 컴파일합니다. PATH 에 없으면 `ZIG=<path>` 로 지정. 이 머신은 `~/.local/zig-0.15.2/zig` |
| cargo-nextest | 최신 | `just test` / `just check` 가 사용 |
| rustup 타깃 `x86_64-pc-windows-msvc` | | `just check` 의 `windows-lint` 가 사용 |
| just, python3, bun | | 유지보수·문서 계약 테스트 |

`just fork-sync` / `just fork-install` 은 `ZIG` 가 비어 있으면 `~/.local/zig-0.15.2/zig` 를 기본값으로 씁니다.

## 리모트와 브랜치

```
origin  https://github.com/herdrdev/herdr.git   # upstream (읽기 전용으로 취급)
fork    git@github.com:lloydkwon/herdr.git      # 개인 포크
```

- `master` = `origin/master` + 포크 커밋들. 포크의 기본 브랜치이며 `fork/master` 로 push 합니다.
- 새 기능은 `master` 에서 브랜치를 따서 작업하고, 끝나면 `master` 에 fast-forward 로 합칩니다(머지 커밋 없이).
- 옛 포크 작업(알림 히스토리, pane 상단 제목)은 `fork/backup/master-before-upstream-sync` 에 보관돼 있습니다.

## 업데이트 절차

```bash
just fork-sync      # git fetch origin → git rebase origin/master → just check
just fork-install   # cargo build --release → ~/.local/bin/herdr-prev.bak 백업 → 설치 → live handoff
```

`fork-sync` 가 충돌로 멈추면:

1. 충돌 파일을 해결합니다. 대부분 «양쪽 다 유지»입니다.
2. `docs/next/api/herdr-api.schema.json` 충돌은 손으로 합치지 말고 해결을 마친 뒤 재생성합니다:
   `HERDR_UPDATE_API_SCHEMA=1 just test-one generated_protocol_schema_artifact_is_current`
3. `src/protocol/wire.rs` 의 `PROTOCOL_VERSION` 이 충돌하면 upstream 이 올린 값 **위로** 포크 bump 를 다시 잡고,
   `tests/api_ping.rs`, `tests/support/mod.rs`, `tests/cli/sessions.rs` 의 기대값을 같은 값으로 맞춥니다.
4. `git add … && GIT_EDITOR=true git rebase --continue`, 남은 커밋도 같은 식으로 처리합니다.
5. `just check` 가 통과하면 `git push --force-with-lease fork master`.

`fork-install` 이 끝나면 붙어 있던 TUI 클라이언트는 종료됩니다. 터미널에서 `herdr` 를 다시 실행하기만 하세요.
(핸드오프 자동 재접속이 들어간 빌드부터는 클라이언트가 그대로 새 서버에 붙습니다.)

## 복구

- 실수로 upstream 빌드로 덮어썼다면: `just fork-install` 을 다시 실행합니다(`target/release` 가 남아 있으면 수 초).
- 직전 바이너리로 되돌리려면: `cp -p ~/.local/bin/herdr-prev.bak ~/.local/bin/herdr` 뒤
  `herdr server live-handoff --import-exe ~/.local/bin/herdr`.

## 세션 안에서 디버그 빌드 확인

기존 herdr 세션 안에서 디버그 빌드를 시험할 때는 상속된 소켓 override 를 지웁니다:

```bash
env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH cargo run -- <command>
```

격리된 헤드리스 서버에 TUI 클라이언트를 붙여 볼 때는 nested 감지 변수를 지우고 PTY 크기를 줘야 합니다:

```bash
env -u HERDR_ENV -u HERDR_PANE_ID -u HERDR_TAB_ID -u HERDR_WORKSPACE_ID -u HERDR_BIN_PATH \
  script -qfc "stty cols 120 rows 40; ./target/debug/herdr" /dev/null
```

유닉스 소켓 경로는 짧아야 하므로(`sun_path` 제한) 격리 디렉터리는 `/tmp/hcheck-*` 처럼 짧게 둡니다.
