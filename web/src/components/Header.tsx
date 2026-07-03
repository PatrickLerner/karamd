export function Header({
  date,
  newLink,
  onToggleMenu,
}: {
  date: string;
  newLink: string;
  onToggleMenu: () => void;
}) {
  return (
    <header className="app-header">
      <div className="header-left">
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
      </div>
      <div className="header-right">
        <span className="header-date">{date}</span>
        <a href={newLink} className="header-action header-new">
          + New
        </a>
      </div>
    </header>
  );
}
