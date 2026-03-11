import { useEffect, useRef, useState } from "react";

function useInView() {
  const ref = useRef<HTMLDivElement>(null);
  const [inView, setInView] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setInView(true);
          observer.disconnect();
        }
      },
      { threshold: 0.2 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return { ref, inView };
}

function Pane({
  visible,
  delay,
  children,
  className = "",
}: {
  visible: boolean;
  delay: number;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`bg-bg-deep rounded-lg border border-border-subtle overflow-hidden transition-all duration-600 ${
        visible
          ? "opacity-100 translate-y-0"
          : "opacity-0 translate-y-4"
      } ${className}`}
      style={{ transitionDelay: `${delay}ms` }}
    >
      {children}
    </div>
  );
}

export default function WorkspaceVisual() {
  const { ref, inView } = useInView();

  return (
    <section
      ref={ref}
      className="py-24 sm:py-32 px-6 flex flex-col items-center"
    >
      <div className="w-full max-w-4xl">
        {/* macOS window chrome */}
        <div className="rounded-xl border border-border-visible bg-bg-elevated overflow-hidden shadow-2xl">
          {/* Title bar */}
          <div className="flex items-center gap-2 px-4 py-3 bg-bg-surface border-b border-border-subtle">
            <div className="flex gap-1.5">
              <div className="w-3 h-3 rounded-full bg-[#ff5f57]" />
              <div className="w-3 h-3 rounded-full bg-[#febc2e]" />
              <div className="w-3 h-3 rounded-full bg-[#28c840]" />
            </div>
            <span className="ml-2 text-xs text-text-tertiary font-mono">
              Tide — ~/projects/my-app
            </span>
          </div>

          {/* 4-pane layout */}
          <div className="grid grid-cols-1 sm:grid-cols-[200px_1fr] grid-rows-[1fr] gap-px bg-border-subtle p-px min-h-[340px]">
            {/* File tree */}
            <Pane visible={inView} delay={0} className="row-span-2 hidden sm:block">
              <div className="p-3 font-mono text-xs leading-relaxed text-text-secondary">
                <div className="text-text-tertiary mb-1">FILES</div>
                <div className="text-accent-blue">▾ src/</div>
                <div className="pl-3">main.rs</div>
                <div className="pl-3 text-accent-cyan">config.rs</div>
                <div className="pl-3">renderer.rs</div>
                <div className="pl-3">terminal.rs</div>
                <div className="text-accent-blue">▾ assets/</div>
                <div className="pl-3">icon.png</div>
                <div className="text-text-tertiary">Cargo.toml</div>
                <div className="text-text-tertiary">README.md</div>
              </div>
            </Pane>

            {/* Right column: top = two terminals, bottom = editor */}
            <div className="flex flex-col gap-px">
              {/* Two terminals side by side */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-px">
                <Pane visible={inView} delay={150}>
                  <div className="p-3 font-mono text-xs leading-relaxed">
                    <div className="text-accent-cyan mb-1">❯ terminal 1</div>
                    <div className="text-text-secondary">
                      $ cargo build --release
                    </div>
                    <div className="text-[#28c840]">
                      &nbsp;&nbsp;Compiling tide v0.1.0
                    </div>
                    <div className="text-[#28c840]">
                      &nbsp;&nbsp;&nbsp;Finished release [optimized]
                    </div>
                    <div className="text-text-secondary">$&nbsp;█</div>
                  </div>
                </Pane>

                <Pane visible={inView} delay={300}>
                  <div className="p-3 font-mono text-xs leading-relaxed">
                    <div className="text-accent-purple mb-1">❯ terminal 2</div>
                    <div className="text-text-secondary">$ git log --oneline</div>
                    <div className="text-text-secondary">
                      <span className="text-accent-blue">a1b2c3d</span> feat:
                      split panes
                    </div>
                    <div className="text-text-secondary">
                      <span className="text-accent-blue">e4f5g6h</span> fix:
                      cursor blink
                    </div>
                    <div className="text-text-secondary">$&nbsp;█</div>
                  </div>
                </Pane>
              </div>

              {/* Editor pane */}
              <Pane visible={inView} delay={450} className="flex-1">
                <div className="p-3 font-mono text-xs leading-relaxed">
                  <div className="text-text-tertiary mb-1">
                    config.rs — editor
                  </div>
                  <div>
                    <span className="text-accent-purple">pub struct</span>{" "}
                    <span className="text-accent-cyan">Config</span>{" "}
                    <span className="text-text-tertiary">{"{"}</span>
                  </div>
                  <div>
                    <span className="text-text-tertiary">&nbsp;&nbsp;</span>
                    <span className="text-accent-blue">font_size</span>
                    <span className="text-text-tertiary">: </span>
                    <span className="text-accent-cyan">f32</span>
                    <span className="text-text-tertiary">,</span>
                  </div>
                  <div>
                    <span className="text-text-tertiary">&nbsp;&nbsp;</span>
                    <span className="text-accent-blue">theme</span>
                    <span className="text-text-tertiary">: </span>
                    <span className="text-accent-cyan">String</span>
                    <span className="text-text-tertiary">,</span>
                  </div>
                  <div>
                    <span className="text-text-tertiary">{"}"}</span>
                  </div>
                </div>
              </Pane>
            </div>
          </div>
        </div>
      </div>

      <p className="mt-8 text-text-secondary text-center text-lg">
        File tree, terminals, editor, browser — all in one window.
      </p>
    </section>
  );
}
