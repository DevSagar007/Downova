import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup, configure } from "@testing-library/react";

// `findBy…`/`waitFor` give up after 1 s by default. The whole suite renders 14 windows in
// parallel, and on a loaded machine (a release build running, a CI runner) a first render can
// take longer, which failed tests that pass on their own. The assertions are unchanged; only how
// long a wait may take is bounded at 5 s.
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});
