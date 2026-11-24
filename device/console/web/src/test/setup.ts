import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Vitest runs without globals (every test imports `describe`/`it` explicitly), so Testing Library
// cannot register its own automatic cleanup. Without this, a second test in the same file finds the
// first test's page still in the document and a query can pass against the wrong render. Registering
// it once here is the reason no test file has to remember; the explicit cleanups that remain are
// harmless.
afterEach(() => cleanup());
