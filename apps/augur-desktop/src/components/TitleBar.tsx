import { useAppStore } from "../store/appStore";

export default function TitleBar() {
  const fileName = useAppStore((s) => s.fileName);
  const caseNumber = useAppStore((s) => s.caseNumber);
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar-left">
        <div className="titlebar-mark">A</div>
        <div className="titlebar-title">AUGUR</div>
      </div>
      <div className="titlebar-center">
        {fileName ? (
          <span>
            {fileName} <span className="titlebar-case">· {caseNumber}</span>
          </span>
        ) : (
          <span className="titlebar-case">{caseNumber}</span>
        )}
      </div>
      <div className="titlebar-right">v1.0.0</div>
    </header>
  );
}
