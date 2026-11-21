import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { errorMessage } from "./errors.js";
import { type RoomSettings, readRoomSettings } from "./room-connection.js";

export function RoomSetup({
  settings,
  inviteError,
  onSave,
}: {
  settings: RoomSettings | undefined;
  inviteError: string | undefined;
  onSave: (settings: RoomSettings) => void;
}) {
  const [feedback, setFeedback] = useState<{
    message: string;
    isError: boolean;
  }>();
  const feedbackElement = useRef<HTMLParagraphElement>(null);
  const validationError = feedback?.isError ?? Boolean(inviteError);

  return (
    <section className="phone-camera">
      <header className="phone-header">
        <h1 id="setup-heading">Room setup</h1>
      </header>
      <p className="phone-intro">
        Set the LiveKit connection for Stream and Room viewer.
      </p>
      <form
        aria-labelledby="setup-heading"
        className="connection-form"
        method="post"
        onInput={() => setFeedback({ message: "", isError: false })}
        onSubmit={(event) => {
          event.preventDefault();
          const fields = new FormData(event.currentTarget);
          try {
            onSave(
              readRoomSettings(
                String(fields.get("serverUrl") ?? ""),
                String(fields.get("token") ?? ""),
                String(fields.get("viewerToken") ?? ""),
              ),
            );
            setFeedback({
              message: "Connection saved.",
              isError: false,
            });
          } catch (error: unknown) {
            setFeedback({
              message: errorMessage(error, "Check the connection details."),
              isError: true,
            });
            feedbackElement.current?.focus();
          }
        }}
      >
        <FieldGroup className="[&_input]:min-h-12 [&_input]:md:text-base">
          <Field>
            <FieldLabel htmlFor="setup-server-url">
              LiveKit server URL
            </FieldLabel>
            <Input
              id="setup-server-url"
              name="serverUrl"
              type="url"
              placeholder="wss://your-project.livekit.cloud"
              defaultValue={settings?.serverUrl}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="setup-token">Connection token</FieldLabel>
            <Input
              id="setup-token"
              name="token"
              type="password"
              autoComplete="off"
              defaultValue={settings?.token.value}
              aria-describedby="setup-token-hint"
              required
            />
            <p className="phone-intro" id="setup-token-hint">
              A token with publishing and subscription permissions works for
              both pages.
            </p>
          </Field>
          <Field>
            <FieldLabel htmlFor="setup-viewer-token">
              Separate viewer token (optional)
            </FieldLabel>
            <Input
              id="setup-viewer-token"
              name="viewerToken"
              type="password"
              autoComplete="off"
              defaultValue={settings?.viewerToken?.value}
              aria-describedby="setup-viewer-token-hint"
            />
            <p className="phone-intro" id="setup-viewer-token-hint">
              Use a viewer token with a different identity to watch while this
              device is streaming.
            </p>
          </Field>
        </FieldGroup>
        <Button type="submit" className="stream-button">
          Save connection
        </Button>
        <p
          className={
            validationError ? "phone-status text-destructive" : "phone-status"
          }
          ref={feedbackElement}
          role="status"
          tabIndex={-1}
        >
          {feedback?.message ?? inviteError}
        </p>
      </form>
    </section>
  );
}
