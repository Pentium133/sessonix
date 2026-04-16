import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import WelcomeWizard from "../components/WelcomeWizard";

vi.mock("../lib/api", () => ({
  detectAgents: vi.fn().mockResolvedValue({ claude: true, codex: true, gemini: false }),
  getSetting: vi.fn().mockResolvedValue(null),
  setSetting: vi.fn().mockResolvedValue(undefined),
  installClaudeHooks: vi.fn().mockResolvedValue(true),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue("/home/user/myproject"),
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  sendNotification: vi.fn(),
}));

import * as api from "../lib/api";
import { open } from "@tauri-apps/plugin-dialog";

describe("WelcomeWizard", () => {
  const onComplete = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders Step 1 with Sessonix heading and Get Started button", () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    expect(screen.getByText("Sessonix")).toBeTruthy();
    expect(screen.getByText("Get Started")).toBeTruthy();
  });

  it("shows 4 step dots", () => {
    const { container } = render(<WelcomeWizard onComplete={onComplete} />);
    expect(container.querySelectorAll(".wizard-dot")).toHaveLength(4);
    expect(container.querySelector(".wizard-dot.active")).toBeTruthy();
  });

  it("Get Started advances to Step 2", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => {
      expect(screen.getByText("Agent Detection")).toBeTruthy();
    });
  });

  it("Step 2 shows detected agents with status", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => {
      expect(screen.getByText("claude")).toBeTruthy();
      expect(screen.getByText("codex")).toBeTruthy();
      expect(screen.getByText("gemini")).toBeTruthy();
    });
  });

  it("Step 2 shows hooks checkbox when claude is found", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => {
      expect(screen.getByLabelText(/Install Claude hooks/i)).toBeTruthy();
    });
  });

  it("Next on Step 2 without hooks advances to Step 3", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    // Claude is found → hooks pre-checked → uncheck it
    await waitFor(() => screen.getByLabelText(/Install Claude hooks/i));
    fireEvent.click(screen.getByLabelText(/Install Claude hooks/i));
    fireEvent.click(screen.getByText("Next"));
    await waitFor(() => {
      expect(screen.getByText("Add a Project")).toBeTruthy();
    });
    expect(api.installClaudeHooks).not.toHaveBeenCalled();
  });

  it("Next with hooks pre-checked installs hooks then advances to Step 3", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    // Claude found → hooks pre-checked → button says "Install Hooks & Next"
    await waitFor(() => screen.getByText(/Install Hooks/));
    expect((screen.getByLabelText(/Install Claude hooks/i) as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByText(/Install Hooks/));
    await waitFor(() => {
      expect(api.installClaudeHooks).toHaveBeenCalled();
      expect(screen.getByText("Add a Project")).toBeTruthy();
    });
  });

  it("Step 3 Choose Folder opens dialog and shows folder name", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => screen.getByText(/Install Hooks/));
    fireEvent.click(screen.getByText(/Install Hooks/));
    await waitFor(() => screen.getByText("Choose Folder"));
    fireEvent.click(screen.getByText("Choose Folder"));
    await waitFor(() => {
      expect(open).toHaveBeenCalledWith({ directory: true, multiple: false });
      expect(screen.getByText("myproject")).toBeTruthy();
    });
  });

  it("Skip for now on Step 3 advances to Step 4 without addProject", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => screen.getByText(/Install Hooks/));
    fireEvent.click(screen.getByText(/Install Hooks/));
    await waitFor(() => screen.getByText("Skip for now"));
    fireEvent.click(screen.getByText("Skip for now"));
    await waitFor(() => {
      expect(screen.getByText("You're all set")).toBeTruthy();
    });
  });

  it("Start Using Sessonix saves setting and calls onComplete", async () => {
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => screen.getByText(/Install Hooks/));
    fireEvent.click(screen.getByText(/Install Hooks/));
    await waitFor(() => screen.getByText("Skip for now"));
    fireEvent.click(screen.getByText("Skip for now"));
    await waitFor(() => screen.getByText("Start Using Sessonix"));
    fireEvent.click(screen.getByText("Start Using Sessonix"));
    await waitFor(() => {
      expect(api.setSetting).toHaveBeenCalledWith("welcome_completed", "true");
      expect(onComplete).toHaveBeenCalled();
    });
  });

  it("Step 2 shows warning when no agents found", async () => {
    vi.mocked(api.detectAgents).mockResolvedValueOnce({ claude: false, codex: false, gemini: false });
    render(<WelcomeWizard onComplete={onComplete} />);
    fireEvent.click(screen.getByText("Get Started"));
    await waitFor(() => {
      expect(screen.getByText(/No AI agents detected/i)).toBeTruthy();
    });
  });
});
