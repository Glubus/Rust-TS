import type { OverlayState } from "../types";

const messages: string[] = [];

export function pushOverlayMessage(message: string): number {
  messages.push(message);
  return messages.length;
}

export function overlayState(): OverlayState {
  return {
    visible: messages.length > 0,
    messages: [...messages],
  };
}
