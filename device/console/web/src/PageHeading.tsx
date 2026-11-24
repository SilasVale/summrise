import type { ReactNode } from "react";

export type PageHeadingProps = {
  title: ReactNode;
  description?: ReactNode;
  kicker?: ReactNode;
  actions?: ReactNode;
  level?: 1 | 2;
};

/** A page's title and primary action, independent of its reading or working area. */
export function PageHeading({ title, description, kicker, actions, level = 1 }: PageHeadingProps) {
  const Heading = level === 1 ? "h1" : "h2";
  return (
    <header className="page-head" data-heading-level={level}>
      <div className="page-heading-copy">
        {kicker && <p className="page-kicker">{kicker}</p>}
        <Heading className="page-title">{title}</Heading>
        {description && <p className="page-description">{description}</p>}
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </header>
  );
}
