import type { Step } from "../types";

const STEPS: { id: Step; label: string }[] = [
  { id: 1, label: "Welcome" },
  { id: 2, label: "Profile" },
  { id: 3, label: "Install" },
  { id: 4, label: "Done" },
];

export default function StepNav({ currentStep }: { currentStep: Step }) {
  return (
    <nav className="step-nav" aria-label="Setup progress">
      {STEPS.map((s, idx) => {
        const status =
          s.id < currentStep
            ? "complete"
            : s.id === currentStep
              ? "current"
              : "upcoming";
        return (
          <div key={s.id} className={`step step-${status}`}>
            <span className="step-marker">
              {status === "complete" ? "✓" : s.id}
            </span>
            <span className="step-label">{s.label}</span>
            {idx < STEPS.length - 1 && (
              <span className="step-divider" aria-hidden="true">
                ›
              </span>
            )}
          </div>
        );
      })}
    </nav>
  );
}
