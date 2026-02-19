function TerminalMockup() {
  return (
    <div className="w-full max-w-4xl mx-auto rounded-xl border border-border overflow-hidden bg-surface">
      {/* Title bar */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
        <div className="flex gap-1.5">
          <div className="w-3 h-3 rounded-full bg-[#FF5F57]" />
          <div className="w-3 h-3 rounded-full bg-[#FEBC2E]" />
          <div className="w-3 h-3 rounded-full bg-[#28C840]" />
        </div>
        <span className="text-text-dim text-xs font-mono ml-2">Tide</span>
      </div>
      {/* Content */}
      <div className="flex h-72">
        {/* File tree */}
        <div className="w-44 border-r border-border p-3 text-xs font-mono shrink-0">
          <div className="text-accent-dim mb-2 text-[10px] uppercase tracking-wider">~/project</div>
          <div className="space-y-1 text-text-dim">
            <div className="text-accent">src/</div>
            <div className="pl-3">main.rs</div>
            <div className="pl-3 text-text">app.rs</div>
            <div className="pl-3">lib.rs</div>
            <div className="text-accent">tests/</div>
            <div>Cargo.toml</div>
            <div>README.md</div>
          </div>
        </div>
        {/* Panes */}
        <div className="flex-1 flex flex-col">
          {/* Top pane */}
          <div className="flex-1 border-b border-border p-3 font-mono text-xs">
            <div className="text-text-dim mb-1">
              <span className="text-accent">~/project</span> <span className="text-[#28C840]">main</span> <span className="text-text-dim">$</span>
            </div>
            <div className="text-text">cargo build --release</div>
            <div className="text-[#28C840] mt-1">Compiling tide v0.1.0</div>
            <div className="text-[#28C840]">Finished `release` profile</div>
          </div>
          {/* Bottom pane */}
          <div className="flex-1 p-3 font-mono text-xs">
            <div className="text-text-dim mb-1">
              <span className="text-accent">~/project</span> <span className="text-[#28C840]">main</span> <span className="text-text-dim">$</span>
            </div>
            <div className="text-text">cargo test</div>
            <div className="text-text-dim mt-1">running 12 tests</div>
            <div className="text-[#28C840]">test result: ok. 12 passed; 0 failed</div>
          </div>
        </div>
        {/* Editor dock */}
        <div className="w-56 border-l border-border shrink-0">
          <div className="text-[10px] font-mono px-3 py-2 border-b border-border text-accent">
            app.rs
          </div>
          <div className="p-3 font-mono text-xs space-y-0.5">
            <div><span className="text-text-dim">1</span> <span className="text-[#C586C0]">use</span> <span className="text-text">std::path</span>;</div>
            <div><span className="text-text-dim">2</span></div>
            <div><span className="text-text-dim">3</span> <span className="text-[#C586C0]">pub fn</span> <span className="text-[#DCDCAA]">run</span>() {'{'}</div>
            <div><span className="text-text-dim">4</span>   <span className="text-[#C586C0]">let</span> app = <span className="text-[#DCDCAA]">App::new</span>();</div>
            <div><span className="text-text-dim">5</span>   app.<span className="text-[#DCDCAA]">start</span>();</div>
            <div><span className="text-text-dim">6</span> {'}'}</div>
          </div>
        </div>
      </div>
    </div>
  )
}

function Feature({ title, description }: { title: string; description: string }) {
  return (
    <div className="p-6 rounded-lg border border-border bg-surface hover:bg-surface-hover transition-colors">
      <h3 className="font-mono text-sm font-semibold text-accent mb-2">{title}</h3>
      <p className="text-sm text-text-dim leading-relaxed">{description}</p>
    </div>
  )
}

function Kbd({ children }: { children: string }) {
  return (
    <kbd className="inline-block px-1.5 py-0.5 rounded bg-surface border border-border font-mono text-[11px] text-text-dim">
      {children}
    </kbd>
  )
}

function KeybindingRow({ keys, action }: { keys: string; action: string }) {
  return (
    <div className="flex items-center justify-between py-2 border-b border-border last:border-0">
      <span className="text-sm text-text-dim">{action}</span>
      <span className="flex gap-1">
        {keys.split(' / ').map((k, i) => (
          <Kbd key={i}>{k}</Kbd>
        ))}
      </span>
    </div>
  )
}

export function App() {
  return (
    <div className="min-h-screen">
      {/* Hero */}
      <section className="max-w-4xl mx-auto px-6 pt-32 pb-20">
        <h1 className="font-mono text-5xl font-bold tracking-tight mb-6">
          Tide
        </h1>
        <p className="text-xl text-text-dim max-w-xl leading-relaxed mb-4">
          A terminal that doesn't make you leave.
        </p>
        <p className="text-sm text-text-dim max-w-lg leading-relaxed mb-12">
          GPU-rendered native terminal with split panes, file tree, and editor in one window.
          Built with Rust and wgpu. No Electron.
        </p>
        <TerminalMockup />
      </section>

      {/* Why */}
      <section className="max-w-4xl mx-auto px-6 py-20">
        <h2 className="font-mono text-sm font-semibold text-accent-dim uppercase tracking-wider mb-4">Why</h2>
        <p className="text-lg text-text-dim max-w-lg leading-relaxed">
          You open an editor to read a file. A finder to browse directories.
          Another window for diffs. One task, but your context is scattered across three apps.
        </p>
        <p className="text-lg text-text max-w-lg leading-relaxed mt-4">
          Tide keeps that context in one screen.
        </p>
      </section>

      {/* Features */}
      <section className="max-w-4xl mx-auto px-6 py-20">
        <h2 className="font-mono text-sm font-semibold text-accent-dim uppercase tracking-wider mb-8">Features</h2>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Feature
            title="Split Panes"
            description="Split horizontally or vertically. Drag borders to resize. Each pane runs its own shell with independent scrollback and working directory. Switch to stacked mode for tab-based navigation."
          />
          <Feature
            title="File Tree"
            description="Follows the focused terminal's working directory. Real-time filesystem watching. Git status badges. Click a file to open it in the editor dock."
          />
          <Feature
            title="Editor Dock"
            description="View and edit files right next to your terminal. Syntax highlighting, search, git diff view, and disk change detection."
          />
          <Feature
            title="Focus System"
            description="Cmd+1/2/3 toggles between File Tree, Pane Area, and Editor Dock. Each key cycles through three states: show + focus, focus, hide."
          />
          <Feature
            title="Drag & Drop"
            description="Drag panes to rearrange your layout. Drop zones for top, bottom, left, right, or swap. The layout tree restructures automatically."
          />
          <Feature
            title="Session Restore"
            description="Layout, open tabs, split ratios, and focus state are saved automatically and restored on next launch."
          />
        </div>
      </section>

      {/* Keybindings */}
      <section className="max-w-4xl mx-auto px-6 py-20">
        <h2 className="font-mono text-sm font-semibold text-accent-dim uppercase tracking-wider mb-8">Keybindings</h2>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
          <div>
            <h3 className="font-mono text-xs text-accent mb-3">Navigation</h3>
            <div className="rounded-lg border border-border bg-surface p-4">
              <KeybindingRow keys="Cmd+1 / 2 / 3" action="Toggle area" />
              <KeybindingRow keys="Cmd+H/J/K/L" action="Navigate within area" />
              <KeybindingRow keys="Cmd+Enter" action="Toggle zoom" />
              <KeybindingRow keys="Cmd+I / Cmd+O" action="Dock tab prev / next" />
            </div>
          </div>
          <div>
            <h3 className="font-mono text-xs text-accent mb-3">Panes</h3>
            <div className="rounded-lg border border-border bg-surface p-4">
              <KeybindingRow keys="Cmd+T" action="Split horizontal" />
              <KeybindingRow keys="Cmd+Shift+T" action="Split vertical" />
              <KeybindingRow keys="Cmd+\" action="Split horizontal (cwd)" />
              <KeybindingRow keys="Cmd+Shift+\" action="Split vertical (cwd)" />
              <KeybindingRow keys="Cmd+W" action="Close pane" />
            </div>
          </div>
        </div>
      </section>

      {/* Tech */}
      <section className="max-w-4xl mx-auto px-6 py-20">
        <h2 className="font-mono text-sm font-semibold text-accent-dim uppercase tracking-wider mb-8">Built with</h2>
        <div className="flex flex-wrap gap-3">
          {['Rust', 'wgpu', 'cosmic-text', 'alacritty_terminal', 'syntect', 'winit', 'notify'].map((tech) => (
            <span
              key={tech}
              className="px-3 py-1.5 rounded-md border border-border bg-surface font-mono text-sm text-text-dim"
            >
              {tech}
            </span>
          ))}
        </div>
      </section>

      {/* Footer */}
      <footer className="max-w-4xl mx-auto px-6 py-12 border-t border-border">
        <p className="text-xs text-text-dim font-mono">Tide</p>
      </footer>
    </div>
  )
}
