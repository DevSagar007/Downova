import type { RefObject } from "react";
import { Search, X } from "lucide-react";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import "./DownloadSearch.css";

/**
 * Compact history search above the list (CORE-10). The field filters local snapshots only; the
 * result count is announced separately after a short delay so typing is not read on every key.
 */
export function DownloadSearch({
  value,
  onChange,
  inputRef,
  resultCount,
}: {
  value: string;
  onChange: (value: string) => void;
  inputRef: RefObject<HTMLInputElement | null>;
  resultCount: number;
}) {
  const { t } = usePreferences();
  const active = value.trim() !== "";
  return (
    <div className="download-search" role="search" aria-label={t("search.label")}>
      <label className="download-search__label" htmlFor="download-search">
        {t("search.label")}
      </label>
      <div className="download-search__field">
        <Search className="icon download-search__icon" aria-hidden="true" />
        <input
          ref={inputRef}
          id="download-search"
          className="download-search__input"
          type="search"
          value={value}
          placeholder={t("search.placeholder")}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Escape" || value === "") return;
            event.preventDefault();
            event.stopPropagation();
            onChange("");
          }}
        />
        {active && (
          <button
            type="button"
            className="download-search__clear"
            aria-label={t("search.clear")}
            title={t("search.clear")}
            onClick={() => {
              onChange("");
              inputRef.current?.focus();
            }}
          >
            <X className="icon" aria-hidden="true" />
          </button>
        )}
      </div>
      {active && (
        <span className="download-search__count" aria-hidden="true">
          {resultCount === 0
            ? t("search.noMatchesShort")
            : format(t("search.matchesShort"), { count: resultCount })}
        </span>
      )}
    </div>
  );
}
