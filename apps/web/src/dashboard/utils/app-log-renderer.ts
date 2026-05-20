import type { ComposerTranslation } from "vue-i18n";
import type { AppLogEvent, JsonValue, Provider } from "../api/client.ts";
import {
  entityToken as buildEntityToken,
  type EntityKind,
  type EntityToken,
} from "../lib/entity-links.ts";

export type AppLogToken = EntityToken;

export interface RenderedAppLog {
  title: AppLogToken[];
  detail: string | null;
}

type PayloadObject = Record<string, JsonValue | undefined>;

type ProviderMap = ReadonlyMap<string, Provider>;

function objectValue(value: JsonValue | undefined): PayloadObject | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as PayloadObject)
    : null;
}

function stringValue(value: JsonValue | undefined): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function numberValue(value: JsonValue | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function booleanValue(value: JsonValue | undefined): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function entityToken(
  kind: EntityKind,
  entity: PayloadObject | null,
  providerById?: ProviderMap,
): AppLogToken {
  const id = stringValue(entity?.id) ?? "";
  const liveLabel = id ? providerById?.get(id)?.name?.trim() : null;
  const fallbackLabel = stringValue(entity?.label) ?? stringValue(entity?.name) ?? undefined;
  return buildEntityToken(
    { kind, id, label: liveLabel ?? fallbackLabel },
    liveLabel ?? fallbackLabel,
  );
}

function sentence(parts: Array<AppLogToken | string>): AppLogToken[] {
  return parts.map((part) => (typeof part === "string" ? { type: "text", text: part } : part));
}

function changeObjects(payload: PayloadObject | null): PayloadObject[] {
  const changes = payload?.changes;
  return Array.isArray(changes)
    ? changes
        .map((change) => objectValue(change))
        .filter((change): change is PayloadObject => !!change)
    : [];
}

function actorSuffix(payload: PayloadObject | null, t: ComposerTranslation): string {
  const actor = objectValue(payload?.actor);
  const type = stringValue(actor?.type);
  if (type === "system") return t("events.actorSystem");
  if (type === "operator") return t("events.actorOperator");
  return "";
}

function credentialChangeDetail(
  payload: PayloadObject | null,
  fallback: string | null,
  t: ComposerTranslation,
): string | null {
  const names = changeObjects(payload)
    .map((change) => stringValue(change.field))
    .filter((field): field is string => !!field)
    .map((field) => t(`events.changeFields.${field}`));
  if (!names.length) return fallback;
  return t("events.changedFields", { fields: names.join(t("events.changeSeparator")) });
}

function legacyCircuitPayload(event: AppLogEvent): PayloadObject | null {
  const match = /^Circuit (opened|recovered|reset): (.+)$/.exec(event.message);
  if (!match) return null;
  const failures = event.detail ? /^(\d+) consecutive failures$/.exec(event.detail)?.[1] : null;
  return {
    subject: { kind: "provider", id: match[2], label: match[2] },
    circuit: {
      consecutive_failures: failures ? Number(failures) : null,
      open_timeout_secs: null,
    },
  };
}

function legacyCircuitType(event: AppLogEvent): string | undefined {
  if (event.event_type !== "legacy.message") return event.event_type;
  if (event.message.startsWith("Circuit opened:")) return "circuit.opened";
  if (event.message.startsWith("Circuit recovered:")) return "circuit.closed";
  if (event.message.startsWith("Circuit reset:")) return "circuit.reset";
  return event.event_type;
}

function renderCircuitEvent(
  event: AppLogEvent,
  t: ComposerTranslation,
  providerById?: ProviderMap,
): RenderedAppLog | null {
  const legacyPayload = legacyCircuitPayload(event);
  const payload = legacyPayload ?? objectValue(event.payload);
  const subject = objectValue(payload?.subject);
  const credential = objectValue(payload?.credential);
  const provider = objectValue(payload?.provider);
  const circuit = objectValue(payload?.circuit);
  const subjectKind = stringValue(subject?.kind) ?? (credential ? "credential" : "provider");
  const subjectToken =
    subjectKind === "credential"
      ? entityToken("credential", credential ?? subject, providerById)
      : entityToken("provider", provider ?? subject, providerById);
  const providerToken = entityToken("provider", provider, providerById);
  const failures = numberValue(circuit?.consecutive_failures);
  const timeoutSecs = numberValue(circuit?.open_timeout_secs);
  const timeoutMins = timeoutSecs === null ? null : Math.max(1, Math.round(timeoutSecs / 60));

  const type = legacyCircuitType(event);

  switch (type) {
    case "circuit.opened": {
      const title =
        subjectKind === "credential" && provider
          ? sentence([
              t("events.provider"),
              " “",
              providerToken,
              "” ",
              t("events.credential"),
              " “",
              subjectToken,
              "” ",
              timeoutMins === null
                ? t("events.circuitOpenedAfterFailuresNoDuration", { count: failures ?? 0 })
                : t("events.circuitOpenedAfterFailures", {
                    count: failures ?? 0,
                    minutes: timeoutMins,
                  }),
            ])
          : sentence([
              t(subjectKind === "credential" ? "events.credential" : "events.provider"),
              " “",
              subjectToken,
              "” ",
              timeoutMins === null
                ? t("events.circuitOpenedAfterFailuresNoDuration", { count: failures ?? 0 })
                : t("events.circuitOpenedAfterFailures", {
                    count: failures ?? 0,
                    minutes: timeoutMins,
                  }),
            ]);
      return { title, detail: event.detail };
    }
    case "circuit.closed":
      return {
        title: sentence(["“", subjectToken, "” ", t("events.circuitRecovered")]),
        detail: event.detail,
      };
    case "circuit.reset":
      return {
        title: sentence(["“", subjectToken, "” ", t("events.circuitReset")]),
        detail: event.detail,
      };
    default:
      return null;
  }
}

function renderProviderEvent(
  event: AppLogEvent,
  t: ComposerTranslation,
  providerById?: ProviderMap,
): RenderedAppLog | null {
  const payload = objectValue(event.payload);
  const provider = objectValue(payload?.provider);
  const providerToken = entityToken("provider", provider, providerById);
  const actionKey = {
    "provider.created": "events.providerCreated",
    "provider.updated": "events.providerUpdated",
    "provider.deleted": "events.providerDeleted",
    "provider.enabled": "events.providerEnabled",
    "provider.disabled": "events.providerDisabled",
  }[event.event_type ?? ""];
  if (!actionKey) return null;

  return {
    title: sentence([t("events.provider"), " “", providerToken, "” ", t(actionKey)]),
    detail: event.detail,
  };
}

function renderCredentialEvent(
  event: AppLogEvent,
  t: ComposerTranslation,
  providerById?: ProviderMap,
): RenderedAppLog | null {
  const payload = objectValue(event.payload);
  const credential = objectValue(payload?.credential);
  const provider = objectValue(payload?.provider);
  const providerId =
    stringValue(provider?.id) ??
    stringValue(credential?.provider_id) ??
    stringValue(payload?.provider_id);
  const credentialToken = entityToken("credential", credential, providerById);
  const providerToken = providerId ? entityToken("provider", provider, providerById) : null;
  const actionKey = {
    "credential.created": "events.credentialCreated",
    "credential.updated": "events.credentialUpdated",
    "credential.deleted": "events.credentialDeleted",
    "credential.enabled": "events.credentialEnabled",
    "credential.disabled": "events.credentialDisabled",
    "credential.auto_disabled": "events.credentialAutoDisabled",
  }[event.event_type ?? ""];
  if (!actionKey) return null;

  const changes = changeObjects(payload);
  const enabledChange = changes.find((change) => stringValue(change.field) === "enabled");
  const enabledTo = booleanValue(enabledChange?.to);
  const effectiveActionKey =
    event.event_type === "credential.updated" && enabledTo === true
      ? "events.credentialEnabled"
      : event.event_type === "credential.updated" && enabledTo === false
        ? "events.credentialDisabled"
        : actionKey;
  const suffix = actorSuffix(payload, t);
  const title = providerToken
    ? sentence([
        t("events.provider"),
        " “",
        providerToken,
        "” ",
        t("events.credential"),
        " “",
        credentialToken,
        "” ",
        t(effectiveActionKey),
        suffix,
      ])
    : sentence([
        t("events.credential"),
        " “",
        credentialToken,
        "” ",
        t(effectiveActionKey),
        suffix,
      ]);
  return { title, detail: credentialChangeDetail(payload, event.detail, t) };
}

export function renderAppLogEvent(
  event: AppLogEvent,
  t: ComposerTranslation,
  providerById?: ProviderMap,
): RenderedAppLog {
  const rendered =
    renderCircuitEvent(event, t, providerById) ??
    renderProviderEvent(event, t, providerById) ??
    renderCredentialEvent(event, t, providerById);
  if (rendered) return rendered;
  return {
    title: [{ type: "text", text: event.message }],
    detail: event.detail,
  };
}
