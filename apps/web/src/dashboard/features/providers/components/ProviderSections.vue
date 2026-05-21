<script setup lang="ts">
import { useI18n } from "vue-i18n";
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderAuthPoolSummary,
  ProviderHealthSummary,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderCard from "./provider-card.vue";
import UiBadge from "../../../components/ui/badge.vue";
import UiCard from "../../../components/ui/card.vue";
import type { ProviderSectionView } from "../types.ts";
import { formatDurationMs } from "../../../utils/format-duration.ts";

const { t } = useI18n();

const props = defineProps<{
  sections: ProviderSectionView[];
  healthMap: Record<string, ProviderHealthSummary>;
  credsByProvider: Record<string, Credential[]>;
  loadingCreds: Record<string, boolean>;
  toggleBusy: Record<string, boolean>;
  circuitResetBusy: Record<string, boolean>;
  credModelRefreshBusy: Record<string, boolean>;
  credBalanceRefreshBusy: Record<string, boolean>;
  credToggleBusy: Record<string, boolean>;
  poolByProviderId: Record<string, ProviderAuthPoolSummary>;
  planSnapByCred: Record<string, CredentialPlanSnapshot | null>;
  activeCredentialCountsByProvider: Record<string, Record<string, number>>;
  providerRollingStatById: Map<string, NonNullable<ProviderHealthSummary["rolling"]>>;
  highlightedProviderId: string | null;
}>();

const emit = defineEmits<{
  refreshCredModels: [credentialId: string];
  refreshCredBalance: [credentialId: string];
  toggleProvider: [provider: Provider];
  resetCircuit: [providerId: string];
  editProvider: [provider: Provider];
  deleteProvider: [providerId: string];
  addCred: [providerId: string];
  toggleCred: [credential: Credential];
  editCred: [credential: Credential];
  deleteCred: [credential: Credential];
}>();

function poolRows(providerId: string): CredentialPoolStatus[] {
  return props.poolByProviderId[providerId]?.credentials ?? [];
}

function tokensPerSec(providerId: string): number | null | undefined {
  return (
    props.providerRollingStatById.get(providerId)?.decode_output_tokens_per_sec ||
    props.providerRollingStatById.get(providerId)?.output_tokens_per_sec
  );
}

function fastestLabel(value: number | null): string {
  return value == null
    ? t("summary.noSpeed")
    : t("summary.bestLatency", { duration: formatDurationMs(value) });
}
</script>

<template>
  <div class="space-y-4">
    <UiCard v-for="section in sections" :key="section.key" class="overflow-hidden">
      <div class="border-b border-border bg-muted/40 px-4 py-4 sm:px-5">
        <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div class="min-w-0 flex-1 space-y-2">
            <div class="flex min-w-0 flex-wrap items-center gap-2">
              <span
                class="inline-flex size-8 items-center justify-center rounded-lg bg-primary/10 text-primary"
              >
                <VpIcon name="layers-3" size-class="size-4" />
              </span>
              <div class="min-w-0">
                <h2 class="truncate text-base font-semibold text-foreground">
                  {{ section.title }}
                </h2>
                <p class="text-xs text-muted-foreground">{{ section.description }}</p>
              </div>
            </div>
            <div class="flex flex-wrap gap-2 text-xs">
              <UiBadge variant="secondary">
                {{
                  t("summary.activeEndpoints", {
                    enabled: section.summary.enabledEndpoints,
                    total: section.summary.totalEndpoints,
                  })
                }}
              </UiBadge>
              <UiBadge v-if="section.summary.blockedCredentials" variant="outline">
                {{ t("summary.blockedCredentials", { count: section.summary.blockedCredentials }) }}
              </UiBadge>
              <UiBadge variant="outline">
                {{
                  t("summary.credentials", {
                    available: section.summary.availableCredentials,
                    enabled: section.summary.enabledCredentials,
                  })
                }}
              </UiBadge>
              <UiBadge variant="outline">
                {{ fastestLabel(section.summary.fastestLatencyMs) }}
              </UiBadge>
              <UiBadge variant="outline">{{
                t("summary.models", { count: section.summary.remoteModels })
              }}</UiBadge>
              <UiBadge variant="outline">
                {{
                  t("summary.nativeBridge", {
                    native: section.summary.nativeEndpoints,
                    bridge: section.summary.bridgedEndpoints,
                  })
                }}
              </UiBadge>
            </div>
          </div>
        </div>
      </div>

      <div
        class="hidden border-b border-border bg-muted/25 px-4 py-2 text-[11px] font-medium uppercase tracking-normal text-muted-foreground xl:grid xl:grid-cols-[minmax(18rem,1.15fr)_minmax(16rem,0.9fr)_minmax(20rem,1.15fr)_auto] xl:items-center"
      >
        <span>{{ t("table.provider") }}</span>
        <span>{{ t("table.routing") }}</span>
        <span>{{ t("table.credentials") }}</span>
        <span class="text-center">{{ t("table.actions") }}</span>
      </div>

      <div class="space-y-3 p-4 sm:p-5 xl:space-y-0 xl:p-0">
        <ProviderCard
          v-for="card in section.providers"
          :id="`provider-${card.provider.id}`"
          :data-provider-id="card.provider.id"
          :key="card.provider.id"
          :card="card"
          :health="healthMap[card.provider.id]"
          :creds="credsByProvider[card.provider.id] ?? []"
          :loading-creds="!!loadingCreds[card.provider.id]"
          :toggle-provider-busy="!!toggleBusy[card.provider.id]"
          :circuit-reset-busy="!!circuitResetBusy[card.provider.id]"
          :cred-model-refresh-busy="credModelRefreshBusy"
          :cred-balance-refresh-busy="credBalanceRefreshBusy"
          :cred-toggle-busy="credToggleBusy"
          :pool-rows="poolRows(card.provider.id)"
          :plan-snap-by-cred="planSnapByCred"
          :active-credential-counts="activeCredentialCountsByProvider[card.provider.id] ?? {}"
          :tokens-per-sec="tokensPerSec(card.provider.id)"
          :class="[
            highlightedProviderId === card.provider.id
              ? 'ring-2 ring-primary/35 ring-offset-2 ring-offset-background'
              : '',
          ]"
          @refresh-cred-models="emit('refreshCredModels', $event)"
          @refresh-cred-balance="emit('refreshCredBalance', $event)"
          @toggle-provider="emit('toggleProvider', $event)"
          @reset-circuit="emit('resetCircuit', $event)"
          @edit-provider="emit('editProvider', $event)"
          @delete-provider="emit('deleteProvider', $event)"
          @add-cred="emit('addCred', $event)"
          @toggle-cred="emit('toggleCred', $event)"
          @edit-cred="emit('editCred', $event)"
          @delete-cred="emit('deleteCred', $event)"
        />
      </div>
    </UiCard>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "summary": {
      "activeEndpoints": "{enabled}/{total} active",
      "bestLatency": "{duration} best",
      "blockedCredentials": "{count} blocked creds",
      "credentials": "{available}/{enabled} creds",
      "models": "{count} models",
      "nativeBridge": "{native} native · {bridge} bridge",
      "noSpeed": "no speed"
    },
    "table": {
      "actions": "Actions",
      "credentials": "Credentials",
      "provider": "Provider",
      "routing": "Routing"
    }
  },
  "zh-CN": {
    "summary": {
      "activeEndpoints": "{enabled}/{total} 已启用",
      "bestLatency": "最快 {duration}",
      "blockedCredentials": "{count} 个凭证受阻",
      "credentials": "{available}/{enabled} 凭证可用",
      "models": "{count} 个模型",
      "nativeBridge": "{native} 原生 · {bridge} 桥接",
      "noSpeed": "无测速"
    },
    "table": {
      "actions": "操作",
      "credentials": "凭证",
      "provider": "供应商",
      "routing": "上游"
    }
  }
}
</i18n>
