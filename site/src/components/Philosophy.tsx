export default function Philosophy() {
  return (
    <section className="py-24 sm:py-32 px-6">
      <div className="max-w-3xl mx-auto text-center">
        <h2 className="text-3xl sm:text-4xl font-bold mb-8">
          Stop switching.{" "}
          <span className="gradient-text">Start flowing.</span>
        </h2>

        <div className="space-y-6 text-text-secondary text-lg leading-relaxed text-left sm:text-center">
          <p>
            Every time you switch between your terminal, editor, and file
            manager, you lose focus. That context switch adds up — minutes
            become hours, and deep work becomes shallow. Tide eliminates that
            friction by putting everything you need in a single window.
          </p>
          <p>
            Built from scratch in Rust with GPU-accelerated rendering, Tide
            isn't a web app pretending to be native. It's a real macOS
            application that starts instantly, uses minimal memory, and renders
            every frame on the GPU. No Electron, no compromises.
          </p>
        </div>
      </div>
    </section>
  );
}
