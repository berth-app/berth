interface Props {
  current: number;
  total: number;
}

export default function StepIndicator({ current, total }: Props) {
  return (
    <div className="flex items-center gap-2">
      {Array.from({ length: total }, (_, i) => (
        <div
          key={i}
          className={`rounded-full transition-all duration-200 ${
            i === current
              ? "w-2.5 h-2.5 bg-berth-accent shadow-berth-glow"
              : i < current
                ? "w-2 h-2 bg-berth-accent/50"
                : "w-2 h-2 bg-berth-surface-3"
          }`}
        />
      ))}
    </div>
  );
}
