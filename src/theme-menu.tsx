import { ChevronDownIcon, MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const themeOptions = {
  system: { label: "System", Icon: MonitorIcon },
  light: { label: "Light", Icon: SunIcon },
  dark: { label: "Dark", Icon: MoonIcon },
};

type ThemePreference = keyof typeof themeOptions;

const documentRoot = document.documentElement;
const storageKey = documentRoot.getAttribute("data-theme-storage-key");
if (!storageKey) throw new Error("Theme storage key is missing");
const themeStorageKey = storageKey;

function parseThemePreference(value: unknown): ThemePreference {
  return value === "light" || value === "dark" ? value : "system";
}

function applyThemePreference(preference: ThemePreference): void {
  if (preference === "system") documentRoot.removeAttribute("data-theme");
  else documentRoot.setAttribute("data-theme", preference);
}

export function useThemePreference() {
  const [theme, setTheme] = useState(() =>
    parseThemePreference(documentRoot.getAttribute("data-theme")),
  );

  useEffect(() => {
    function syncTheme(event: StorageEvent) {
      if (event.key !== themeStorageKey && event.key !== null) return;
      if (event.storageArea !== window.localStorage) return;
      const preference = parseThemePreference(event.newValue);
      applyThemePreference(preference);
      setTheme(preference);
    }

    window.addEventListener("storage", syncTheme);
    return () => window.removeEventListener("storage", syncTheme);
  }, []);

  function changeTheme(preference: ThemePreference) {
    applyThemePreference(preference);
    setTheme(preference);
    try {
      if (preference === "system") localStorage.removeItem(themeStorageKey);
      else localStorage.setItem(themeStorageKey, preference);
    } catch {
      // The current page still honors the selection when storage is unavailable.
    }
  }

  return { theme, changeTheme };
}

export function ThemeMenu({
  theme,
  onThemeChange,
}: {
  theme: ThemePreference;
  onThemeChange: (preference: ThemePreference) => void;
}) {
  const { Icon, label } = themeOptions[theme];
  return (
    <TooltipProvider delay={400}>
      <DropdownMenu modal={false}>
        <Tooltip>
          <TooltipTrigger
            render={
              <DropdownMenuTrigger
                render={
                  <Button
                    className="h-11 min-w-11 gap-1 px-2 text-muted-foreground aria-expanded:bg-accent group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:min-w-0 group-data-[collapsible=icon]:p-0"
                    type="button"
                    variant="ghost"
                    aria-label={`Theme: ${label}`}
                  />
                }
              />
            }
          >
            <Icon aria-hidden="true" />
            <ChevronDownIcon
              className="size-3 group-data-[collapsible=icon]:hidden"
              aria-hidden="true"
            />
          </TooltipTrigger>
          <TooltipContent sideOffset={8}>Change theme</TooltipContent>
        </Tooltip>
        <DropdownMenuContent
          className="min-w-42"
          align="start"
          side="top"
          sideOffset={8}
          collisionPadding={12}
        >
          <DropdownMenuRadioGroup
            aria-label="Color theme"
            value={theme}
            onValueChange={(value) =>
              onThemeChange(parseThemePreference(value))
            }
          >
            {(["system", "light", "dark"] as const).map((preference) => {
              const { Icon, label } = themeOptions[preference];
              return (
                <DropdownMenuRadioItem
                  className="min-h-11"
                  closeOnClick
                  key={preference}
                  value={preference}
                >
                  <Icon aria-hidden="true" />
                  {label}
                </DropdownMenuRadioItem>
              );
            })}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </TooltipProvider>
  );
}
