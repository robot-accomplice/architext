import { useEffect, useRef, useState } from "react";
import {
  readBooleanPreference,
  readDebugRouting,
  readRoutingStylePreference,
  writeBooleanPreference,
  writeRoutingStylePreference
} from "../adapters/browserPreferences.js";
import type { DiagramTransform, Flow, Mode, RoutingStyle, View, ViewportSize } from "../domain/architectureTypes.js";

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

function estimateCanvasSize(mode: Mode, view: View, flow: Flow): ViewportSize {
  if (mode === "sequence") {
    const participantCount = new Set(flow.steps.flatMap((step) => [step.from, step.to])).size;
    return {
      width: 56 + participantCount * 146,
      height: 88 + flow.steps.length * 56 + 56
    };
  }

  if (mode === "c4") {
    return {
      width: Math.max(760, 112 + view.lanes.length * 210),
      height: Math.max(440, 72 + Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * 86 + 96)
    };
  }

  return {
    width: 192 + view.lanes.length * 210,
    height: Math.max(380, 86 + Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * 84 + 104)
  };
}

export function useDiagramViewport({
  compactFitZoom,
  desktopFitZoom,
  localStorage,
  locationSearch,
  windowObject = window
}: {
  compactFitZoom: number;
  desktopFitZoom: number;
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

  const fitZoomFor = (mode: Mode, view: View, flow: Flow) => {
    const estimate = estimateCanvasSize(mode, view, flow);
    const availableWidth = Math.max(diagramViewportSize.width - 24, 1);
    const availableHeight = Math.max(diagramViewportSize.height - 24, 1);
    const nextZoom = Math.min(availableWidth / estimate.width, availableHeight / estimate.height);
    const readableMinimum = windowObject.innerWidth < 900 ? compactFitZoom : desktopFitZoom;
    return Math.min(1, Math.max(readableMinimum, Number(nextZoom.toFixed(2))));
  };

  return {
    debugRouting,
    diagramTransform,
    diagramViewportRef,
    diagramViewportSize,
    fitZoomFor,
    navCollapsed,
    rightCollapsed,
    routingStyle,
    setDiagramTransform,
    setNavCollapsed,
    setRightCollapsed,
    setRoutingStyle
  };
}
