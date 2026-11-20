import { LayoutGridIcon, VideoIcon } from "lucide-react";
import { useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { PhoneCamera } from "./phone.js";
import { RoomViewer } from "./room-viewer.js";
import { ThemeMenu, useThemePreference } from "./theme-menu.js";
import "./theme.css";
import "./phone.css";

const cameraViews = [
  { id: "publish", label: "Stream", Icon: VideoIcon },
  { id: "room", label: "Room viewer", Icon: LayoutGridIcon },
] as const;

type CameraView = (typeof cameraViews)[number]["id"];

const navigationId = "camera-navigation";
const sourceLabel = "View source on GitHub";

function CameraShell() {
  const { theme, changeTheme } = useThemePreference();
  const [activeView, setActiveView] = useState<CameraView>(() =>
    new URLSearchParams(location.hash.slice(1)).get("view") === "room"
      ? "room"
      : "publish",
  );
  const { isMobile, open, openMobile, setOpenMobile } = useSidebar();
  const navigationTrigger = useRef<HTMLButtonElement>(null);
  const navigationExpanded = isMobile ? openMobile : open;
  const navigationLabel = isMobile
    ? "Open navigation"
    : open
      ? "Collapse navigation"
      : "Expand navigation";

  return (
    <>
      <Sidebar
        id={navigationId}
        collapsible="icon"
        mobileFinalFocus={navigationTrigger}
      >
        <SidebarHeader className="px-2 pt-[max(0.75rem,env(safe-area-inset-top))]">
          <div className="flex min-h-11 min-w-0 items-center gap-2 overflow-hidden px-1 pr-12 md:pr-1 group-data-[collapsible=icon]:px-0">
            <img
              className="size-8 shrink-0 rounded-md"
              src="./hebbian-logo.svg"
              alt=""
            />
            <span className="truncate text-sm font-semibold">
              LiveKit Cameras
            </span>
          </div>
        </SidebarHeader>
        <SidebarContent className="px-2">
          <nav aria-label="Camera tools">
            <SidebarMenu className="gap-1">
              {cameraViews.map(({ id, label, Icon }) => (
                <SidebarMenuItem key={id}>
                  <SidebarMenuButton
                    type="button"
                    aria-current={activeView === id ? "page" : undefined}
                    isActive={activeView === id}
                    aria-label={label}
                    tooltip={label}
                    className="h-11 data-active:font-semibold"
                    onClick={() => {
                      setActiveView(id);
                      setOpenMobile(false);
                    }}
                  >
                    <Icon aria-hidden="true" />
                    <span>{label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </nav>
        </SidebarContent>
        <SidebarFooter className="flex-row items-center gap-1 pb-[max(0.5rem,env(safe-area-inset-bottom))] group-data-[collapsible=icon]:flex-col">
          <SidebarMenu className="w-auto">
            <SidebarMenuItem>
              <SidebarMenuButton
                aria-label={sourceLabel}
                tooltip={sourceLabel}
                className="size-11 justify-center text-muted-foreground"
                render={
                  <a href="https://github.com/Hebbian-Robotics/livekit-cameras" />
                }
              >
                <svg viewBox="0 0 98 96" fill="currentColor" aria-hidden="true">
                  <path d="M41.4395 69.3848C28.8066 67.8535 19.9062 58.7617 19.9062 46.9902C19.9062 42.2051 21.6289 37.0371 24.5 33.5918C23.2559 30.4336 23.4473 23.7344 24.8828 20.959C28.7109 20.4805 33.8789 22.4902 36.9414 25.2656C40.5781 24.1172 44.4062 23.543 49.0957 23.543C53.7852 23.543 57.6133 24.1172 61.0586 25.1699C64.0254 22.4902 69.2891 20.4805 73.1172 20.959C74.457 23.543 74.6484 30.2422 73.4043 33.4961C76.4668 37.1328 78.0937 42.0137 78.0937 46.9902C78.0937 58.7617 69.1934 67.6621 56.3691 69.2891C59.623 71.3945 61.8242 75.9883 61.8242 81.252L61.8242 91.2051C61.8242 94.0762 64.2168 95.7031 67.0879 94.5547C84.4102 87.9512 98 70.6289 98 49.1914C98 22.1074 75.9883 6.69539e-07 48.9043 4.309e-07C21.8203 1.92261e-07 -1.9479e-07 22.1074 -4.3343e-07 49.1914C-6.20631e-07 70.4375 13.4941 88.0469 31.6777 94.6504C34.2617 95.6074 36.75 93.8848 36.75 91.3008L36.75 83.6445C35.4102 84.2188 33.6875 84.6016 32.1562 84.6016C25.8398 84.6016 22.1074 81.1563 19.4277 74.7441C18.375 72.1602 17.2266 70.6289 15.0254 70.3418C13.877 70.2461 13.4941 69.7676 13.4941 69.1934C13.4941 68.0449 15.4082 67.1836 17.3223 67.1836C20.0977 67.1836 22.4902 68.9063 24.9785 72.4473C26.8926 75.2227 28.9023 76.4668 31.2949 76.4668C33.6875 76.4668 35.2187 75.6055 37.4199 73.4043C39.0469 71.7773 40.291 70.3418 41.4395 69.3848Z" />
                </svg>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
          <ThemeMenu theme={theme} onThemeChange={changeTheme} />
        </SidebarFooter>
      </Sidebar>

      <SidebarInset className="min-w-0">
        <div className="camera-toolbar">
          <SidebarTrigger
            ref={navigationTrigger}
            aria-controls={!isMobile || openMobile ? navigationId : undefined}
            aria-expanded={navigationExpanded}
            aria-haspopup={isMobile ? "dialog" : undefined}
            aria-label={navigationLabel}
            className="size-11"
          />
        </div>
        <div hidden={activeView !== "publish"}>
          <PhoneCamera />
        </div>
        <div hidden={activeView !== "room"}>
          <RoomViewer />
        </div>
      </SidebarInset>
    </>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Camera tools root is missing");
createRoot(root).render(
  <TooltipProvider>
    <SidebarProvider>
      <CameraShell />
    </SidebarProvider>
  </TooltipProvider>,
);
