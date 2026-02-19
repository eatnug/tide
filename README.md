# Tide

A terminal that doesn't make you leave.

## What

터미널에서 작업하다 보면 흐름이 끊기는 순간이 있다. 파일 하나 보려고 VS Code를 열고, 디렉토리 구조 확인하려고 Finder를 켜고, diff 보려고 또 다른 창을 띄운다. 하나의 작업인데 맥락이 세 곳으로 흩어진다.

Tide는 그 맥락을 한 화면 안에 둔다. 터미널 옆에 파일 트리가 있고, 파일을 클릭하면 에디터가 열리고, 터미널은 그 자리에 그대로 있다. 앱을 전환할 일이 없다.

장기적으로는 터미널을 중심으로 한 통합 워크스페이스를 지향한다. [Wave Terminal](https://waveterm.dev)이 웹 기술로 하고 있는 것을 네이티브 GPU 렌더링으로.

## Core Ideas

- **맥락을 깨지 않는다** — 파일을 보거나 편집할 때 터미널을 떠나지 않는다
- **터미널이 중심이다** — IDE가 아니다. 터미널에 필요한 것만 붙인다
- **네이티브 성능** — Electron 없이, wgpu로 직접 GPU 렌더링

## Features

### Split Panes

터미널을 가로/세로로 분할한다. 보더를 드래그해서 크기를 조절한다. 각 pane은 독립적인 셸, 스크롤백, 작업 디렉토리를 가진다.

Stacked 모드로 전환하면 탭 바 형태로 한 pane만 보여준다.

### File Tree

포커스된 터미널의 작업 디렉토리를 따라간다. 터미널 포커스를 옮기면 트리도 바뀐다.

- 파일시스템 감시로 실시간 반영
- Git status 뱃지
- 클릭하면 에디터 독에서 열림

### Editor Dock

터미널 옆에서 파일을 보고 편집한다.

- 신택스 하이라이팅
- 검색
- Git diff 뷰
- 디스크 변경 감지 (외부에서 파일이 바뀌면 알림)
- 탭으로 여러 파일 관리

### Focus System

세 영역을 `Cmd+1/2/3`으로 전환한다.

| Key | Area |
|---|---|
| `Cmd+1` | File Tree |
| `Cmd+2` | Pane Area |
| `Cmd+3` | Editor Dock |

각 키는 **show + focus → focus → hide** 세 단계를 토글한다. `Cmd+H/J/K/L`로 영역 안에서 이동하고, `Cmd+Enter`로 풀스크린 줌.

### Drag & Drop

Pane을 드래그해서 레이아웃을 재배치한다. 상하좌우 드롭 존 + 스왑.

### Session Restore

레이아웃, 열린 탭, 분할 비율, 포커스 상태를 자동 저장하고 다음 실행 시 복원한다.

## Keybindings

`~/.config/tide/settings.json`에서 커스터마이즈 가능.

### Navigation

| Key | Action |
|---|---|
| `Cmd+1` / `2` / `3` | 영역 토글 |
| `Cmd+H/J/K/L` | 영역 내 이동 |
| `Cmd+Enter` | 줌 토글 |
| `Cmd+I` / `Cmd+O` | 독 탭 이전 / 다음 |

### Panes

| Key | Action |
|---|---|
| `Cmd+T` | 가로 분할 (홈) |
| `Cmd+Shift+T` | 세로 분할 (홈) |
| `Cmd+\` | 가로 분할 (cwd) |
| `Cmd+Shift+\` | 세로 분할 (cwd) |
| `Cmd+W` | Pane 닫기 |

### General

| Key | Action |
|---|---|
| `Cmd+Shift+O` | 파일 파인더 |
| `Cmd+F` | 터미널 검색 |
| `Cmd+Shift+D` | 다크 / 라이트 토글 |
| `Cmd+=` / `Cmd+-` | 폰트 크기 조절 |
| `Cmd+,` | 설정 |

## Tech Stack

| | |
|---|---|
| Language | Rust |
| GPU | wgpu |
| Text | cosmic-text + CoreText fallback |
| Terminal | alacritty_terminal |
| Syntax | syntect |
| Window | winit |
| File watch | notify |

## Architecture

```
tide/
  crates/
    tide-core/        shared types, traits
    tide-renderer/    wgpu GPU rendering
    tide-terminal/    PTY, terminal emulation
    tide-layout/      split pane layout engine
    tide-tree/        file tree
    tide-input/       keybinding, input routing
    tide-editor/      editor, diff viewer
    tide-app/         app entry point
```

## Build

```sh
cargo build --release                    # binary
cargo bundle --release -p tide-app       # macOS .app bundle
```