import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import AgentIcon from "../components/AgentIcon";

describe("AgentIcon", () => {
  describe("opencode", () => {
    it("renders an SVG element for opencode", () => {
      const { container } = render(<AgentIcon agentType="opencode" />);
      const svg = container.querySelector("svg");
      expect(svg).not.toBeNull();
    });

    it("uses the --opencode color token", () => {
      const { container } = render(<AgentIcon agentType="opencode" />);
      const svg = container.querySelector("svg");
      expect(svg).not.toBeNull();
      const markup = svg!.outerHTML;
      expect(markup).toContain("var(--opencode)");
    });

    it("respects the size prop", () => {
      const { container } = render(<AgentIcon agentType="opencode" size={24} />);
      const svg = container.querySelector("svg");
      expect(svg).not.toBeNull();
      expect(svg!.getAttribute("width")).toBe("24");
      expect(svg!.getAttribute("height")).toBe("24");
    });

    it("renders a distinct icon (not the custom fallback)", () => {
      const opencodeRender = render(<AgentIcon agentType="opencode" />);
      const customRender = render(<AgentIcon agentType="custom" />);
      const opencodeSvg = opencodeRender.container.querySelector("svg")!.innerHTML;
      const customSvg = customRender.container.querySelector("svg")!.innerHTML;
      expect(opencodeSvg).not.toBe(customSvg);
    });
  });
});
