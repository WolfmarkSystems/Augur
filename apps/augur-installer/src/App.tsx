import { useState } from "react";
import StepNav from "./components/StepNav";
import ProfileSelect from "./components/ProfileSelect";
import InstallProgress from "./components/InstallProgress";
import Complete from "./components/Complete";
import type { InstallResult, Profile, Step } from "./types";

function Header() {
  return (
    <header className="wiz-header">
      <div className="wiz-brand">
        <div className="wiz-brand-mark">A</div>
        <div className="wiz-brand-text">AUGUR Setup Wizard</div>
      </div>
      <div className="wiz-version">v1.0.0</div>
    </header>
  );
}

function Footer({
  step,
  onBack,
  onNext,
  profile,
  installRunning,
}: {
  step: Step;
  onBack: () => void;
  onNext: () => void;
  profile: Profile;
  installRunning: boolean;
}) {
  // Footer button labels change with the step.
  const nextLabel = step === 2 ? "Install →" : step === 3 ? "Working…" : "Done";
  const nextDisabled = step === 3 || step === 4;
  return (
    <footer className="wiz-footer">
      <div className="wiz-footer-status">
        {step === 2 && (
          <span className="wiz-footer-hint">
            Profile selected: <strong>{profile}</strong>
          </span>
        )}
        {step === 3 && installRunning && (
          <span className="wiz-footer-hint">Installing components…</span>
        )}
        {step === 4 && (
          <span className="wiz-footer-hint">Installation complete</span>
        )}
      </div>
      <div className="wiz-footer-buttons">
        <button
          type="button"
          className="btn btn-secondary"
          onClick={onBack}
          disabled={step <= 2 || installRunning || step === 4}
        >
          Back
        </button>
        <button
          type="button"
          className="btn btn-primary"
          onClick={onNext}
          disabled={nextDisabled}
        >
          {nextLabel}
        </button>
      </div>
    </footer>
  );
}

export default function App() {
  const [step, setStep] = useState<Step>(2);
  const [profile, setProfile] = useState<Profile>("standard");
  const [installResult, setInstallResult] = useState<InstallResult | null>(
    null,
  );
  const [installRunning, setInstallRunning] = useState(false);

  return (
    <div className="wizard">
      <Header />
      <StepNav currentStep={step} />
      <div className="wiz-body">
        {step === 2 && (
          <ProfileSelect selected={profile} onSelect={setProfile} />
        )}
        {step === 3 && (
          <InstallProgress
            profile={profile}
            onStart={() => setInstallRunning(true)}
            onComplete={(result) => {
              setInstallResult(result);
              setInstallRunning(false);
              setStep(4);
            }}
          />
        )}
        {step === 4 && installResult && <Complete result={installResult} />}
      </div>
      <Footer
        step={step}
        installRunning={installRunning}
        onBack={() => setStep((s) => Math.max(2, s - 1) as Step)}
        onNext={() => setStep((s) => Math.min(4, s + 1) as Step)}
        profile={profile}
      />
    </div>
  );
}
