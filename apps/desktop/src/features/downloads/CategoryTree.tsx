import { useState } from "react";
import { ChevronDown, ChevronRight, Folder, FolderOpen, ListOrdered, X } from "lucide-react";
import { usePreferences } from "../../app/preferences";
import { CATEGORY_TREE, type CategoryId, type CategoryNode } from "./categories";
import "./CategoryTree.css";

export interface CategoryTreeProps {
  selected: CategoryId;
  onSelect: (id: CategoryId) => void;
  /** Hides the panel (View → Show categories brings it back). */
  onClose: () => void;
}

/** Left-hand category panel. Parents expand/collapse; the selected node has aria-current. */
export function CategoryTree({ selected, onSelect, onClose }: CategoryTreeProps) {
  const { t } = usePreferences();
  const [collapsed, setCollapsed] = useState<ReadonlySet<CategoryId>>(new Set());

  const toggle = (id: CategoryId) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const renderNode = (node: CategoryNode, depth: number) => {
    const hasChildren = Boolean(node.children?.length);
    const open = !collapsed.has(node.id);
    const isSelected = selected === node.id;
    const label = t(node.labelKey);
    const NodeIcon =
      node.id === "queues" || node.id === "main-queue"
        ? ListOrdered
        : isSelected
          ? FolderOpen
          : Folder;
    return (
      <li key={node.id}>
        <div className="category-tree__row" data-depth={depth}>
          {hasChildren ? (
            <button
              type="button"
              className="category-tree__toggle"
              aria-expanded={open}
              aria-label={`${open ? t("categories.collapse") : t("categories.expand")}: ${label}`}
              onClick={() => toggle(node.id)}
            >
              {open ? (
                <ChevronDown className="icon" aria-hidden="true" />
              ) : (
                <ChevronRight className="icon" aria-hidden="true" />
              )}
            </button>
          ) : (
            <span className="category-tree__spacer" aria-hidden="true" />
          )}
          <button
            type="button"
            className="category-tree__item"
            aria-current={isSelected ? "true" : undefined}
            onClick={() => onSelect(node.id)}
          >
            <NodeIcon className="icon category-tree__icon" aria-hidden="true" />
            <span className="category-tree__label">{label}</span>
          </button>
        </div>
        {hasChildren && open && (
          <ul className="category-tree__list">
            {node.children?.map((child) => renderNode(child, depth + 1))}
          </ul>
        )}
      </li>
    );
  };

  return (
    <nav className="category-tree" aria-label={t("categories.label")}>
      <div className="category-tree__header">
        <h2 className="category-tree__heading">{t("categories.label")}</h2>
        <button
          type="button"
          className="category-tree__close"
          aria-label={t("categories.hide")}
          title={t("categories.hide")}
          onClick={onClose}
        >
          <X className="icon" aria-hidden="true" />
        </button>
      </div>
      <ul className="category-tree__list category-tree__list--root">
        {CATEGORY_TREE.map((node) => renderNode(node, 0))}
      </ul>
    </nav>
  );
}
