interface PlaceholderPageProps {
  title: string;
  phase: string;
}

/** Skeleton page for a center whose implementation lands in a later phase. */
export default function PlaceholderPage({ title, phase }: PlaceholderPageProps) {
  return (
    <section>
      <h1>{title}</h1>
      <p className="muted">该中心将在 {phase} 实现，当前仅保留导航与页面骨架。</p>
    </section>
  );
}
