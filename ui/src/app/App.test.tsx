import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  it("renders OctoFHIR Server UI heading", () => {
    render(<App />);
    const heading = screen.getByRole("heading", { name: /octofhir server ui/i });
    expect(heading).toBeInTheDocument();
  });
});
