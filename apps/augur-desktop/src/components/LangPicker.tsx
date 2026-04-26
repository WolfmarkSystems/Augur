import { useEffect, useMemo, useRef, useState } from "react";
import type { Language, Tier } from "../types";
import { ALL_LANGUAGES } from "./languages";

const TIER_ORDER: Tier[] = ["High quality", "Forensic priority", "Limited quality"];

const QUALITY_LABEL: Record<Language["quality"], string> = {
  hi: "high",
  med: "med",
  low: "low",
};

interface Props {
  selected: Language;
  role: "source" | "target";
  trailingNote?: string;
  onSelect: (lang: Language) => void;
}

export default function LangPicker({ selected, role, trailingNote, onSelect }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);

  // ESC + outside click close.
  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const handleClick = (e: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener("keydown", handleKey);
    document.addEventListener("mousedown", handleClick);
    return () => {
      document.removeEventListener("keydown", handleKey);
      document.removeEventListener("mousedown", handleClick);
    };
  }, [open]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return ALL_LANGUAGES;
    return ALL_LANGUAGES.filter((l) => {
      return (
        l.code.toLowerCase().includes(q) ||
        l.name.toLowerCase().includes(q) ||
        (l.sub ?? "").toLowerCase().includes(q)
      );
    });
  }, [query]);

  const grouped = useMemo(() => {
    const map = new Map<Tier, Language[]>();
    TIER_ORDER.forEach((t) => map.set(t, []));
    filtered.forEach((l) => map.get(l.tier)?.push(l));
    return TIER_ORDER.map((t) => ({ tier: t, items: map.get(t) ?? [] })).filter(
      (g) => g.items.length > 0,
    );
  }, [filtered]);

  return (
    <div
      ref={containerRef}
      className={`langpicker langpicker-${role}`}
      data-open={open}
    >
      <button
        type="button"
        className="langpicker-button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-haspopup="listbox"
      >
        <span className="lp-flag">{selected.flag}</span>
        <span className="lp-name">{selected.name}</span>
        <span className="lp-code">/ {selected.code}</span>
        {trailingNote && <span className="lp-note">· {trailingNote}</span>}
        {!trailingNote && role === "target" && (
          <span className="lp-note">· target</span>
        )}
        <span className="lp-caret" aria-hidden="true">▾</span>
      </button>
      {open && (
        <div
          className={`langpicker-dropdown lp-dd-${role}`}
          role="listbox"
          aria-label="Language list"
        >
          <div className="lp-search">
            <input
              type="text"
              autoFocus
              placeholder="Search languages…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <div className="lp-list">
            {grouped.map(({ tier, items }) => (
              <div key={tier} className="lp-group">
                <div className="lp-group-header">{tier}</div>
                {items.map((l) => {
                  const active = l.code === selected.code;
                  return (
                    <button
                      key={l.code}
                      type="button"
                      className={`lp-row ${active ? "is-active" : ""}`}
                      onClick={() => {
                        onSelect(l);
                        setOpen(false);
                        setQuery("");
                      }}
                      role="option"
                      aria-selected={active}
                    >
                      <span className="lp-row-flag">{l.flag}</span>
                      <span className="lp-row-text">
                        <span className="lp-row-name">{l.name}</span>
                        {l.sub && (
                          <span className="lp-row-sub">{l.sub}</span>
                        )}
                      </span>
                      <span
                        className={`lp-quality lp-quality-${l.quality}`}
                        title={`${QUALITY_LABEL[l.quality]} quality`}
                      >
                        {QUALITY_LABEL[l.quality]}
                      </span>
                      <span className="lp-check" aria-hidden="true">
                        {active ? "✓" : ""}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))}
            {grouped.length === 0 && (
              <div className="lp-empty">
                No languages match “{query}”.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
