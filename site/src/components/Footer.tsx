export default function Footer() {
  return (
    <footer className="py-8 px-6 border-t border-border-subtle">
      <div className="max-w-4xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-4 text-sm text-text-tertiary">
        <span>Tide — MIT License</span>
        <a
          href="https://github.com/eatnug/tide"
          className="hover:text-text-secondary transition-colors"
        >
          GitHub
        </a>
      </div>
    </footer>
  );
}
