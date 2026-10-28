/** 加载态：3 行骨架，不用 spinner（UI-SPEC §6） */
export function Skeleton() {
  return (
    <div className="qw-skeleton" aria-hidden="true">
      {[0, 1, 2].map((row) => (
        <div key={row} className="qw-skeleton-row" />
      ))}
    </div>
  );
}
