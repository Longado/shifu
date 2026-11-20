/** Per-terminal ordering survives slow IPC, while a disconnected attachment invalidates pending keys. */
export class TerminalInput {
  private tail: Promise<unknown> = Promise.resolve();
  private generation = 0;
  private ready = this.waitForAttachment();

  private waitForAttachment() {
    let resolve!: (id: number | null) => void;
    const promise = new Promise<number | null>((done) => { resolve = done; });
    return { promise, resolve };
  }

  attach(id: number): void { this.ready.resolve(id); }

  invalidate(): void {
    this.generation++;
    this.ready.resolve(null);
    this.ready = this.waitForAttachment();
  }

  enqueue(run: (current: () => boolean, id: number) => Promise<unknown>): Promise<unknown> {
    const generation = this.generation;
    const current = () => generation === this.generation;
    const ready = this.ready.promise;
    const result = this.tail.then(async () => {
      if (!current()) return;
      const id = await ready;
      if (id !== null && current()) return run(current, id);
    });
    this.tail = result.catch(() => undefined);
    return result;
  }
}
