export default function Hero() {
  return (
    <section className="relative min-h-screen flex flex-col items-center justify-center px-6 overflow-hidden">
      {/* Radial glow behind icon */}
      <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] rounded-full bg-accent-blue/10 blur-[120px] animate-glow-pulse pointer-events-none" />

      <div className="relative z-10 flex flex-col items-center text-center max-w-3xl">
        <img
          src="/icon.png"
          alt="Tide"
          className="w-28 h-28 mb-8 drop-shadow-[0_0_40px_rgba(59,130,246,0.3)] animate-fade-in-up"
        />

        <h1 className="text-5xl sm:text-7xl font-bold tracking-tight mb-4 animate-fade-in-up stagger-1">
          Tide
        </h1>

        <p className="text-lg sm:text-xl text-text-secondary mb-6 animate-fade-in-up stagger-2">
          A GPU-rendered terminal workspace for macOS
        </p>

        <p className="text-3xl sm:text-4xl font-semibold mb-6 animate-fade-in-up stagger-3">
          Everything you need.{" "}
          <span className="gradient-text">One window.</span>
        </p>

        <p className="text-text-secondary max-w-xl mb-10 animate-fade-in-up stagger-4">
          Tide brings your file tree, terminals, editor, and browser into a
          single GPU-accelerated workspace. Built with Rust. No Electron. No
          context switching.
        </p>

        <div className="flex flex-col sm:flex-row gap-4 animate-fade-in-up stagger-4">
          <a
            href="https://github.com/eatnug/tide/releases"
            className="inline-flex items-center gap-2 px-8 py-3 rounded-xl bg-accent-blue text-white font-medium hover:bg-accent-blue/90 transition-colors"
          >
            <svg
              className="w-5 h-5"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
              />
            </svg>
            Download for macOS
          </a>
          <a
            href="https://github.com/eatnug/tide"
            className="inline-flex items-center gap-2 px-8 py-3 rounded-xl border border-border-visible text-text-primary font-medium hover:bg-bg-elevated transition-colors"
          >
            <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
              <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
            </svg>
            View on GitHub
          </a>
        </div>
      </div>
    </section>
  );
}
