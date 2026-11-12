import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";

vi.mock("@octofhir/ui-kit", () => ({
  Chart: () => null,
  DataTable: () => null,
  MarkdownView: () => null,
}));
import { CellOutput } from "../components/CellOutput";

it("isolates untrusted HTML from the authenticated origin and external resources", () => {
  const html = renderToStaticMarkup(
    createElement(CellOutput, {
      output: {
        kind: "html",
        html: '<img src="/admin" onerror="fetch(\'/admin\')"><script>fetch("/admin")</script>',
      },
      scope: {},
    })
  );
  expect(html).toContain("<iframe");
  expect(html).toContain('sandbox=""');
  expect(html).toContain('referrerPolicy="no-referrer"');
  expect(html).toContain("default-src &#x27;none&#x27;");
  expect(html).not.toContain("allow-scripts");
  expect(html).not.toContain("allow-same-origin");
  expect(html).not.toContain("<script>");
});
