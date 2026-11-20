/** Poll one JSON source at a time. An edit during a read asks for one fresh snapshot. */
export function progressReader<T>(read: () => Promise<T>, show: (value: T) => void, report: (error: unknown) => void) {
  let reading = false;
  let edited = false;
  let shown: string | undefined;
  return async (afterEdit = false): Promise<void> => {
    if (afterEdit) shown = undefined;
    if (reading) {
      if (afterEdit) edited = true;
      return;
    }
    reading = true;
    try {
      do {
        edited = false;
        try {
          const value = await read();
          if (edited) continue;
          const snapshot = JSON.stringify(value);
          if (snapshot !== shown) {
            show(value);
            shown = snapshot;
          }
        } catch (error) {
          if (!edited) report(error);
        }
      } while (edited);
    } finally {
      reading = false;
    }
  };
}
