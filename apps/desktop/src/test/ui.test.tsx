import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { category, relativeTime, shortId, STATUSES_FOR } from "../lib/format";
import { Badge, ConfirmModal, FreshnessBadge, StateBadge, Toggle } from "../components/ui";

describe("format", () => {
  it("derives the ten memory categories from kind + status", () => {
    expect(category("task", "pending")).toBe("Pending task");
    expect(category("task", "completed")).toBe("Completed task");
    expect(category("bug", "open")).toBe("Known bug");
    expect(category("bug", "resolved")).toBe("Resolved bug");
    expect(category("research", "open")).toBe("Open question");
    expect(category("decision", "active")).toBe("Decision");
  });
  it("only offers statuses valid for each kind", () => {
    expect(STATUSES_FOR.decision).toEqual(["active"]);
    expect(STATUSES_FOR.task).toContain("in_progress");
    expect(STATUSES_FOR.bug).not.toContain("completed");
  });
  it("formats relative times", () => {
    const now = Date.parse("2026-11-13T12:00:00Z");
    expect(relativeTime("2026-11-13T11:59:50Z", now)).toBe("just now");
    expect(relativeTime("2026-11-13T09:00:00Z", now)).toBe("3h ago");
    expect(relativeTime(null, now)).toBe("—");
    expect(shortId("mem_01abcdefghijklmnop")).toBe("mem_01abcdef…");
  });
});

describe("components", () => {
  it("labels freshness and integration states", () => {
    render(<><FreshnessBadge value="potentially_stale" /><StateBadge state="pending_user_action" /><Badge>x</Badge></>);
    expect(screen.getByText("Possibly stale")).toBeInTheDocument();
    expect(screen.getByText("Action needed")).toBeInTheDocument();
  });
  it("toggle is an accessible switch", () => {
    const onChange = vi.fn();
    render(<Toggle label="Store prompt text" checked={false} onChange={onChange} />);
    const sw = screen.getByRole("switch", { name: "Store prompt text" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    expect(onChange).toHaveBeenCalledWith(true);
  });
  it("confirm dialog confirms and closes on Escape", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(<ConfirmModal open title="Delete?" body="Really" confirmLabel="Delete" onConfirm={onConfirm} onClose={onClose} />);
    expect(screen.getByRole("dialog", { name: "Delete?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
