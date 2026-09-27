// One-off notices in the toast corner, beside the DayZ update posts (row 14, approved,
// D-303): a settings file that could not be read, a damaged cache moved aside, a join
// that could not be added to Recent. Each says something that happened once; it stays
// until dismissed, and Escape clears the stack as it does the posts.

export type Notice = { id: number; title: string; text: string };

class Notices {
  items = $state<Notice[]>([]);
  #next = 1;

  /** At most three, newest last, like the posts; the same text is not shown twice. */
  push(title: string, text: string) {
    if (this.items.some((n) => n.title === title && n.text === text)) return;
    this.items = [...this.items, { id: this.#next++, title, text }].slice(-3);
  }

  dismiss(id: number) {
    this.items = this.items.filter((n) => n.id !== id);
  }

  clear() {
    this.items = [];
  }
}

export const notices = new Notices();
