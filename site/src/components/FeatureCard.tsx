export default function FeatureCard({
  icon,
  title,
  description,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
}) {
  return (
    <div className="group p-6 rounded-xl bg-bg-elevated border border-border-subtle hover:border-border-visible transition-colors">
      <div className="w-10 h-10 rounded-lg bg-bg-surface flex items-center justify-center text-accent-blue mb-4 group-hover:text-accent-cyan transition-colors">
        {icon}
      </div>
      <h3 className="text-lg font-semibold mb-2">{title}</h3>
      <p className="text-text-secondary text-sm leading-relaxed">
        {description}
      </p>
    </div>
  );
}
