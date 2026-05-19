export function subscribeToDataEvents({
  onValid,
  onInvalid,
  EventSourceCtor = globalThis.EventSource
}) {
  if (!EventSourceCtor) return () => {};
  const source = new EventSourceCtor("/api/data-events");
  source.onmessage = (event) => {
    const payload = JSON.parse(event.data);
    if (payload.type === "valid") onValid(payload);
    else if (payload.type === "invalid") onInvalid(payload);
  };
  source.onerror = () => {
    source.close();
  };
  return () => source.close();
}
