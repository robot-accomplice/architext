import { useEffect, useRef, useState } from "react";
import {
  readBooleanPreference,
  readDebugRouting,
  readRoutingStylePreference,
  writeBooleanPreference,
  writeRoutingStylePreference
} from "../adapters/browserPreferences.js";
import { measuredDiagramFitZoom } from "./diagramFit.js";
import type { DiagramTransform, RoutingStyle, ViewportSize } from "../domain/architectureTypes.js";

function useElementSize<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [size, setSize] = useState<ViewportSize>({ width: 0, height: 0 });

  useEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry.contentRect;
      setSize({ width: box.width, height: box.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return [ref, size] as const;
}

export function useDiagramViewport({
  localStorage,
  locationSearch,
  windowObject = window
}: {
  localStorage: Storage;
  locationSearch: string;
  windowObject?: Window;
}) {
  const [navCollapsed, setNavCollapsed] = useState(() => readBooleanPreference(localStorage, "architext-left-collapsed"));
  const [rightCollapsed, setRightCollapsed] = useState(() => readBooleanPreference(localStorage, "architext-right-collapsed"));
  const [diagramTransform, setDiagramTransform] = useState<DiagramTransform>({ zoom: 1, focused: false });
  const [routingStyle, setRoutingStyle] = useState<RoutingStyle>(() => readRoutingStylePreference(localStorage) as RoutingStyle);
  const [debugRouting] = useState(() => readDebugRouting(locationSearch));
  const [diagramViewportRef, diagramViewportSize] = useElementSize<HTMLElement>();

  useEffect(() => {
    writeBooleanPreference(localStorage, "architext-left-collapsed", navCollapsed);
  }, [localStorage, navCollapsed]);

  useEffect(() => {
    writeBooleanPreference(localStorage, "architext-right-collapsed", rightCollapsed);
  }, [localStorage, rightCollapsed]);

  useEffect(() => {
    writeRoutingStylePreference(localStorage, routingStyle);
  }, [localStorage, routingStyle]);

  useEffect(() => {
    const narrowWidth = windowObject.matchMedia("(max-width: 760px)");
    const laptopWidth = windowObject.matchMedia("(max-width: 1180px)");
    const collapseForViewport = () => {
      if (narrowWidth.matches) {
        setNavCollapsed(true);
        setRightCollapsed(true);
      } else if (laptopWidth.matches) {
        setRightCollapsed(true);
      }
    };

    collapseForViewport();
    narrowWidth.addEventListener("change", collapseForViewport);
    laptopWidth.addEventListener("change", collapseForViewport);
    return () => {
      narrowWidth.removeEventListener("change", collapseForViewport);
      laptopWidth.removeEventListener("change", collapseForViewport);
    };
  }, [windowObject]);

  const fitDisplayedDiagram = () => {
    setDiagramTransform((value) => ({
      ...value,
      zoom: measuredDiagramFitZoom(diagramViewportRef.current)
    }));
  };

  return {
    debugRouting,
    diagramTransform,
    diagramViewportRef,
    diagramViewportSize,
    fitDisplayedDiagram,
    navCollapsed,
    rightCollapsed,
    routingStyle,
    setDiagramTransform,
    setNavCollapsed,
    setRightCollapsed,
    setRoutingStyle
  };
}
