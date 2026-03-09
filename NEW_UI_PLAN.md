# Tide New UI Plan

## 1. 데이터 모델

### 위계

```
App
├── workspaces: Vec<Workspace>
├── active_workspace: usize
├── panes: HashMap<PaneId, PaneKind>   ← 전역 저장소 (워크스페이스 간 이동 가능)
├── sidebar_mode: SidebarMode          ← Hidden | Visible
├── file_tree_mode: FileTreeMode       ← Hidden | Overlay | Pinned
├── file_tree_cwd: PathBuf             ← 마지막 유효 cwd (파일트리용)
└── zoomed_pane: Option<PaneId>        ← 줌 상태

Workspace
├── name: String
├── layout: SplitTree                  ← cwd 없음, 이름만
└── focused_tab_group: LeafId

SplitTree (바이너리 트리)
└── Node
    ├── Leaf(TabGroup)
    └── Split { direction, ratio, left, right }

TabGroup
├── tabs: Vec<PaneId>
└── active: usize

PaneKind
├── Launcher                           ← 새 탭/스플릿 시 타입 선택 화면
├── Terminal { cwd, git_info, backend, ... }
├── Editor { path, content, ... }
├── Browser { url, ... }
└── Diff { left_path, right_path, ... }
```

### 설계 결정

- **Workspace에 cwd 없음**: 워크스페이스는 그룹핑 단위일 뿐. 새 터미널은 마지막 포커스된 터미널의 cwd 상속.
- **리프는 항상 TabGroup**: 단일 패널 = tabs가 1개인 TabGroup. 데이터 구조는 동일, 시각적 표현만 다름.
- **Pane은 워크스페이스 종속**: 다른 워크스페이스로 이동 가능. 나중에 글로벌 워크스페이스 추가 가능.
- **Launcher PaneKind**: 새 탭/스플릿 시 바로 터미널이 아니라 선택 화면이 뜸. 선택하면 해당 타입으로 변환.
  - 옵션: `[T]` Terminal, `[E]` New File, `[O]` Open File, `[B]` Browser
  - 단축키로 즉시 선택
- **파일트리 cwd 추론 규칙**:
  1. 포커스 패널이 Terminal → 그 터미널의 cwd
  2. 포커스 패널이 Editor/Diff → 파일의 부모 디렉토리
  3. 포커스 패널이 Browser/Launcher → 변경 안 함 (이전 cwd 유지)
- **워크스페이스 영속성**: 1단계에서는 비영속(윈도우 닫으면 사라짐). 나중에 레이아웃/cwd/스크롤백 저장/복원 추가.
- **워크스페이스는 윈도우 독립**: 각 윈도우가 자기만의 워크스페이스 세트를 가짐.

---

## 2. 포커스 모델

### 포커스 대상

```
영구 포커스 (영역으로 존재, 명시적 이동):
  1. Panel (항상)
  2. WorkspaceSidebar (Visible 모드일 때)
  3. FileTree (Pinned 모드일 때)

일시적 포커스 (오버레이, 닫히면 자동 복귀):
  - WorkspaceSidebar (Hidden 모드에서 토글 시)
  - FileTree (Overlay 모드에서 토글 시)
  - FileFinder
```

### 규칙

- 포커스는 기본적으로 Panel에 있음
- 사이드바/파일트리의 **표시 모드에 따라** 포커스 행동이 달라짐:
  - Hidden → 토글하면 오버레이(일시적), 선택/ESC로 닫히고 Panel 복귀
  - Visible/Pinned → 영역(영구), 명시적으로 포커스 이동
- 줌 상태: 일반 ↔ 포커스 패널 확대 (2단계)

---

## 3. 키바인딩

### 글로벌 (포커스 무관)

| 키 | 액션 |
|----|------|
| `Cmd+1~9` | 워크스페이스 전환 |
| `Cmd+Shift+N` | 새 워크스페이스 |
| `Cmd+Shift+W` | 워크스페이스 닫기 |
| `Cmd+Enter` | 줌 토글 (포커스 패널 확대/축소) |
| `Cmd+Shift+O` | 파일 파인더 (일시적 오버레이) |
| `Cmd+E` | 파일트리 토글 |

### Panel 포커스

| 키 | 액션 |
|----|------|
| `Cmd+HJKL` | 스플릿 간 이동 (방향) |
| `Cmd+Shift+H/L` | 탭 그룹 내 이전/다음 탭 |
| `Cmd+T` | 현재 탭 그룹에 런처 탭 추가 |
| `Cmd+D` | 새 세로 스플릿 + 런처 |
| `Cmd+Shift+D` | 새 가로 스플릿 + 런처 |
| `Cmd+W` | 현재 탭 닫기 |
| (나머지) | 패널에 전달 (터미널 입력 등) |

### Sidebar/FileTree 포커스

| 키 | 액션 |
|----|------|
| `j/k` | 탐색 (위/아래) |
| `Enter` | 선택 |
| `ESC` | Panel로 복귀 (일시적 모드면 닫힘) |

---

## 4. 시각 요소

### 레이아웃 원칙

- **모든 영역은 독립된 라운드 사각형** (cornerRadius: 6)
- **균일한 패딩**: 타이틀바↔컨텐츠, 좌/우/하단 모두 **6px**
- **패널 간 갭**: **4px** (사이드바↔파일트리, 파일트리↔패널, 패널↔패널 모두 동일)
- 사이드바, 파일트리, 각 패널이 모두 같은 규칙으로 배치 → 계층 없이 평등한 영역들

### 컬러 팔레트

| 변수 | 색상 | 용도 |
|------|------|------|
| `$surface-bg` | `#0A0A0B` | 최외곽 배경 (갭에 보이는 색) |
| `$pane-bg` | `#0E0E10` | 패널 배경 |
| `$file-tree-bg` | `#111113` | 사이드바/파일트리 배경 |
| `$border-focused` | `#C4B8A680` | 포커스 패널 보더 |
| `$border-subtle` | `#1F1F23` | 비포커스 패널 보더 |
| `$accent` | `#C4B8A6` | 포커스 탭 바텀 보더 |
| `$tab-text` | `#6B6B70` | 비활성 탭 텍스트 |
| `$tab-text-focused` | `#FFFFFF` | 활성 탭 텍스트 |
| `$close-icon` | `#4A4A4E` | 탭 닫기 버튼 |

### 컴포넌트 목록

| 컴포넌트 | 설명 | 상태 |
|----------|------|------|
| **Titlebar** | 트래픽라이트, 타이틀, 컨트롤 | TODO: 사이드바 토글 버튼 위치 |
| **Workspace Sidebar** | 2단계(Hidden/Visible). Visible=~180px, 이름/branch/cwd | 확정 |
| **File Tree** | 독립 라운드 사각형, 사이드바 오른쪽. `$file-tree-bg` 동일 배경 | 확정 |
| **Tab Bar** | 탭 그룹 상단, 항상 표시 (탭 1개여도). height: 32px | 확정 |
| **Tab** | 이름 + 닫기(×), 포커스 패널의 활성 탭만 accent 바텀보더(2px) | 확정 |
| **Panel** | 라운드 사각형, `$pane-bg` 배경, cornerRadius: 6 | 확정 |
| **Panel Border** | 포커스: `$border-focused` 2px / 비포커스: `$border-subtle` 1px | 확정 |
| **Split Handle** | 스플릿 경계 드래그 영역 | TODO: 시각적 표현 미정 |
| **Launcher** | [T]Terminal [E]NewFile [O]OpenFile [B]Browser, 중앙 정렬 | 확정 |
| **File Finder** | 검색 오버레이 | 기존과 동일 |

### 포커스 시각 표현

- 포커스된 탭 그룹의 **액티브 탭만** accent 바텀 보더 (2px `$accent` 직사각형)
- 비포커스 탭 그룹의 액티브 탭은 보더 없음, 텍스트 밝기만 차이
- 포커스된 패널 보더: `$border-focused` 2px
- 비포커스 패널 보더: `$border-subtle` 1px

### Sidebar 상세

- **Hidden**: 너비 0px. 타이틀바에 워크스페이스 아이콘+이름 버튼 표시.
- **Visible**: ~180px. 각 워크스페이스 항목:
  - 활성: `$pane-bg` 배경 + `$border-focused` 1px 보더, 이름(bold, white) / branch / cwd
  - 비활성: 배경 없음, 이름(medium, dim) / branch / cwd
  - 하단: "+ New Workspace" 버튼
- 축소(collapsed/icon-only) 모드 **없음**. Hidden ↔ Visible 2단계만.

### File Tree 상세

- 사이드바 오른쪽에 독립 라운드 사각형으로 배치 (같은 `$file-tree-bg` 배경)
- 사이드바↔파일트리 사이 갭 = 파일트리↔패널 사이 갭 (4px, `$surface-bg`)
- 오버레이 모드: 패널 영역 위에 겹침, 패널은 opacity 0.4로 dimmed
- 파일트리가 열려도 사이드바/파일트리/패널 간 시각적 계층 없음 → 동일한 갭+라운드로 평등 배치

---

## 5. 마우스 인터랙션

| 대상 | 액션 |
|------|------|
| 탭 클릭 | 탭 활성화 |
| 탭 닫기 버튼 | 탭 닫기 |
| 탭 드래그 → 같은 탭바 | 순서 변경 |
| 탭 드래그 → 다른 탭바 | 탭 그룹 간 이동 |
| 탭 드래그 → 패널 가장자리 | 새 스플릿으로 분리 |
| 스플릿 핸들 드래그 | 비율 조정 |
| 사이드바 보더 드래그 | 사이드바 너비 조정 |

---

## 6. 애니메이션

| 대상 | 애니메이션 |
|------|-----------|
| 워크스페이스 전환 | 없음. 즉시 전환 |
| 사이드바 보임/숨김 | 너비 슬라이드 (~150ms) |
| 파일트리 보임/숨김 | 너비 슬라이드 (~150ms) |

---

## 7. AS-IS → TO-BE 변경 요약

### 제거

| 항목 | 이유 |
|------|------|
| `EditorDock` (우측 독 영역) | 에디터가 스플릿 트리 안의 탭으로 이동 |
| `TerminalPane.editors/active_editor` | 터미널-에디터 바인딩 제거 |
| `FocusArea::EditorDock` | 3영역 → 2영역 |
| `PaneAreaMode::Stacked` | 탭 그룹이 이 역할을 대체 |
| `editor_panel_maximized/pane_area_maximized` | `zoomed_pane: Option<PaneId>`로 통합 |
| `dock_side`, `sidebar_side` | 사이드바는 항상 왼쪽 |
| 독 관련 드래그 타겟 | PanelTab, PanelBorder 등 |
| 축소(collapsed) 사이드바 | Hidden/Visible 2단계만 |

### 추가

| 항목 | 설명 |
|------|------|
| `Workspace` 구조체 | name, layout, focused_tab_group |
| `TabGroup` 구조체 | tabs, active |
| `PaneKind::Launcher` | 타입 선택 화면 ([T]/[E]/[O]/[B]) |
| `SidebarMode` enum | Hidden / Visible |
| `FileTreeMode` enum | Hidden / Overlay / Pinned |
| 워크스페이스 사이드바 렌더링 | 2단계 |
| 탭 그룹별 탭바 렌더링 | 각 스플릿 리프마다 |
| 파일트리 독립 패널 렌더링 | 라운드 사각형, 사이드바 오른쪽 |

### 유지

- `PaneKind` enum 기본 타입들 (Terminal, Editor, Diff, Browser)
- `panes: HashMap<PaneId, PaneKind>` 전역 저장소
- 개별 패널 렌더링 (grid, text, webview)
- PTY 백엔드 (alacritty_terminal)
- 검색, 선택, 스크롤 등 패널 내부 기능
- IME 처리

---

## 8. 마이그레이션 단계

### Phase 1: 데이터 구조 (기반)
1. `TabGroup` 구조체 추가 (`tide-layout`)
2. `Node::Leaf(PaneId)` → `Node::Leaf(TabGroup)` 변경
3. SplitLayout API 업데이트
4. `Workspace` 구조체 추가 (`tide-app`)
5. App 상태 마이그레이션 (layout/focused → Workspace 내부)

### Phase 2: EditorDock 제거
6. `TerminalPane.editors/active_editor` 제거
7. EditorDock 관련 App 상태 필드 제거
8. `FocusArea` 2영역으로 변경
9. 에디터/브라우저 열기를 탭 그룹 삽입으로 변경

### Phase 3: Layout Compute 재작성
10. `compute_layout()` 단순화 (사이드바 + 스플릿만)
11. 줌 로직 단순화
12. 탭 그룹별 탭바 공간 반영

### Phase 4: 렌더링 재작성
13. EditorDock 크롬 제거
14. 탭 그룹 탭바 렌더링
15. 워크스페이스 사이드바 렌더링
16. 파일트리 독립 패널 렌더링

### Phase 5: 키바인딩 & 네비게이션
17. 키바인딩 매핑 업데이트
18. 포커스 네비게이션 (스플릿 간 Cmd+HJKL, 탭 간 Cmd+Shift+H/L)
19. 워크스페이스 전환 (Cmd+1~9)
20. Launcher PaneKind 구현

### Phase 6: Drag & Drop
21. 탭 드래그 (그룹 내/간 이동, 새 스플릿 분리)
22. 독 관련 드래그 타겟 제거

---

## 9. 미결 사항 (TODO)

- [ ] 스플릿 핸들 시각적 표현 (gap만? hover시 accent 라인?)
- [ ] 타이틀바 컨트롤 버튼 재설계 (기존 3영역 토글 → 새 UI 맞춤)

---

## 10. 디자인 레퍼런스

목업 파일: `/Users/eatnug/Workspace/tide/ui.pen` (Pencil)

| 화면 | 설명 |
|------|------|
| New UI: Normal Mode | Visible 사이드바(180px) + 좌 터미널(포커스) + 우 에디터/브라우저 스플릿 |
| New UI: Zoom Mode | Visible 사이드바 + 줌 터미널 (글로우 효과) |
| New UI: File Tree Overlay | 사이드바 + 파일트리 + 3패널 스플릿(dimmed). 모두 독립 라운드 사각형, 4px 갭 |
| New UI: Sidebar Hidden | 사이드바 없음 (타이틀바에 워크스페이스 버튼) + 풀 터미널 |
| New UI: Launcher | Visible 사이드바 + 터미널 + 런처 패인 ([T]/[E]/[O]/[B]) |

참고: [cmux](https://www.cmux.dev/) — 워크스페이스 사이드바 + 스플릿 + 세션 복원
