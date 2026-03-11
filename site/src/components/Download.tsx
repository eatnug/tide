export default function Download() {
  return (
    <section className="py-24 sm:py-32 px-6">
      <div className="max-w-2xl mx-auto text-center">
        <h2 className="text-3xl sm:text-4xl font-bold mb-4">
          Ready to try Tide?
        </h2>
        <p className="text-text-secondary mb-10 text-lg">
          Download the latest release and see what one window can do.
        </p>

        <a
          href="https://github.com/eatnug/tide/releases"
          className="inline-flex items-center gap-2 px-10 py-4 rounded-xl bg-accent-blue text-white font-medium text-lg hover:bg-accent-blue/90 transition-colors"
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

        <p className="mt-6 text-text-tertiary text-sm">
          Requires macOS 13 Ventura or later. Apple Silicon and Intel supported.
        </p>
      </div>
    </section>
  );
}
