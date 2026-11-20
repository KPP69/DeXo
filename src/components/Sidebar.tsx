interface SidebarProps {
  pages: readonly string[];
  active: string;
  onSelect: (page: string) => void;
}

export default function Sidebar({ pages, active, onSelect }: SidebarProps) {
  return (
    <nav className="sidebar">
      <div className="brand">DeXo</div>
      {pages.map((page) => (
        <button
          key={page}
          type="button"
          className={page === active ? "nav-item active" : "nav-item"}
          onClick={() => onSelect(page)}
        >
          {page}
        </button>
      ))}
    </nav>
  );
}
