import { cn } from "@/lib/utils";

interface Props<T extends string | number> {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  className?: string;
}

/** A small macOS-style segmented control (radio group). */
export function Segmented<T extends string | number>({
  label,
  value,
  options,
  onChange,
  className,
}: Props<T>) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cn("inline-flex items-center gap-0.5 rounded-md bg-muted/60 p-0.5", className)}
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "tabular rounded-[5px] px-2 py-0.5 text-xs",
            value === o.value
              ? "bg-background text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export const START_AT_OPTIONS = [0, 0.25, 0.4].map((v) => ({
  value: v,
  label: `${Math.round(v * 100)}%`,
}));
