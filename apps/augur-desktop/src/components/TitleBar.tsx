import { useAppStore } from "../store/appStore";

export default function TitleBar() {
  const fileName = useAppStore((s) => s.fileName);
  const caseNumber = useAppStore((s) => s.caseNumber);
  const setCaseNumber = useAppStore((s) => s.setCaseNumber);

  const promptCase = () => {
    const next = window.prompt(
      "Case number for exports and chain of custody:",
      caseNumber,
    );
    if (next && next.trim()) setCaseNumber(next.trim());
  };

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar-left">
        <div className="titlebar-mark">A</div>
        <div className="titlebar-title">AUGUR</div>
      </div>
      <div className="titlebar-center">
        {caseNumber ? (
          <button
            type="button"
            className="titlebar-case-btn"
            onClick={promptCase}
            title="Click to change case number"
          >
            Case <strong>{caseNumber}</strong>
          </button>
        ) : (
          <button
            type="button"
            className="titlebar-case-btn titlebar-case-btn-empty"
            onClick={promptCase}
          >
            No case set — set case
          </button>
        )}
        {fileName && (
          <span className="titlebar-file">— {fileName}</span>
        )}
      </div>
      <div className="titlebar-right">v1.0.0</div>
    </header>
  );
}
