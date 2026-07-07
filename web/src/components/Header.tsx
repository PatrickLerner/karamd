export function Header({
  date,
  version,
  onToggleMenu,
}: {
  date: string;
  version: string;
  onToggleMenu: () => void;
}) {
  return (
    <header className="app-header">
      {/* brand cell mirrors the sidebar column so the header sits on the same
          grid as the body below it */}
      <div className="header-brand">
        <button
          type="button"
          className="burger"
          aria-label="Toggle menu"
          onClick={onToggleMenu}
        >
          ☰
        </button>
        <a href="#/" className="brand">
          karamd
        </a>
        {version && <span className="brand-version">v{version}</span>}
      </div>
      <span className="header-date">{date}</span>
    </header>
  );
}
