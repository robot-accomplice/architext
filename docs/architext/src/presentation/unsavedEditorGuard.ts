import { useCallback, useEffect, useMemo } from "react";

export type UnsavedEditorState = {
  id: string;
  label: string;
  dirty: boolean;
};

const discardMessage = (dirtyEditors: UnsavedEditorState[]) => {
  const labels = dirtyEditors.map((editor) => editor.label).join(", ");
  return `You have unsaved changes in ${labels}. Discard those changes and continue?`;
};

export function useUnsavedEditorGuard(editors: UnsavedEditorState[]) {
  const dirtyEditors = useMemo(
    () => editors.filter((editor) => editor.dirty),
    [editors]
  );
  const hasUnsavedChanges = dirtyEditors.length > 0;

  useEffect(() => {
    if (!hasUnsavedChanges) return undefined;
    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", beforeUnload);
    return () => window.removeEventListener("beforeunload", beforeUnload);
  }, [hasUnsavedChanges]);

  const confirmEditorNavigation = useCallback(() => {
    if (!dirtyEditors.length) return true;
    return window.confirm(discardMessage(dirtyEditors));
  }, [dirtyEditors]);

  return {
    confirmEditorNavigation,
    hasUnsavedChanges
  };
}
