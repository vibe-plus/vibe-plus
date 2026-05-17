<script setup lang="ts">
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderAuthPoolSummary,
  ProviderHealthSummary,
  RequestRuntimeStats,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderCard from "./provider-card.vue";
import UiBadge from "../../../components/ui/badge.vue";
import UiButton from "../../../components/ui/button.vue";
import UiCard from "../../../components/ui/card.vue";
import type { ProviderSectionView } from "../types.ts";

const props = defineProps<{
  sections: ProviderSectionView[];
  healthMap: Record<string, ProviderHealthSummary>;
  credsByProvider: Record<string, Credential[]>;
  loadingCreds: Record<string, boolean>;
  toggleBusy: Record<string, boolean>;
  circuitResetBusy: Record<string, boolean>;
  speedtestBusy: Record<string, boolean>;
  modelRefreshBusy: Record<string, boolean>;
  credModelRefreshBusy: Record<string, boolean>;
  credBalanceRefreshBusy: Record<string, boolean>;
  credToggleBusy: Record<string, boolean>;
  poolByProviderId: Record<string, ProviderAuthPoolSummary>;
  planSnapByCred: Record<string, CredentialPlanSnapshot | null>;
  activeCredentialCountsByProvider: Record<string, Record<string, number>>;
  activeRequestCountsByProvider: Map<string, number>;
  liveTokensPerSecByProvider: Map<string, number>;
  providerRollingStatById: Map<string, NonNullable<ProviderHealthSummary["rolling"]>>;
  detectVendorBusy: Record<string, boolean>;
  highlightedProviderId: string | null;
}>();

const emit = defineEmits<{
  speedtestProviders: [providerIds: string[]];
  refreshProviderModelsForProviders: [providerIds: string[]];
  syncCreds: [providerId: string];
  detectVendor: [providerId: string];
  speedtestProvider: [providerId: string];
  refreshModels: [providerId: string];
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
  viewLogs: [providerId: string];
}>();

function providerIdsFromSection(section: ProviderSectionView): string[] {
  return section.providers.map((card) => card.provider.id);
}

function sectionSpeedtestBusy(section: ProviderSectionView): boolean {
  return providerIdsFromSection(section).some((providerId) => !!props.speedtestBusy[providerId]);
}

function sectionModelRefreshBusy(section: ProviderSectionView): boolean {
  return providerIdsFromSection(section).some((providerId) => !!props.modelRefreshBusy[providerId]);
}

function poolRows(providerId: string): CredentialPoolStatus[] {
  return props.poolByProviderId[providerId]?.credentials ?? [];
}

function tokensPerSec(providerId: string): number | null | undefined {
  return (
    props.liveTokensPerSecByProvider.get(providerId) ||
    props.providerRollingStatById.get(providerId)?.decode_output_tokens_per_sec ||
    props.providerRollingStatById.get(providerId)?.output_tokens_per_sec
  );
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
                {{ section.summary.enabledEndpoints }}/{{ section.summary.totalEndpoints }} active
              </UiBadge>
              <UiBadge v-if="section.summary.activeRequests" variant="default">
                live {{ section.summary.activeRequests }}
              </UiBadge>
              <UiBadge v-if="section.summary.blockedCredentials" variant="outline">
                {{ section.summary.blockedCredentials }} blocked creds
              </UiBadge>
              <UiBadge variant="outline">
                {{ section.summary.availableCredentials }}/{{ section.summary.enabledCredentials }}
                creds
              </UiBadge>
              <UiBadge variant="outline">
                {{
                  section.summary.fastestLatencyMs == null
                    ? "no speed"
                    : `${Math.round(section.summary.fastestLatencyMs)}ms best`
                }}
              </UiBadge>
              <UiBadge variant="outline">{{ section.summary.remoteModels }} models</UiBadge>
              <UiBadge variant="outline">
                {{ section.summary.nativeEndpoints }} native ·
                {{ section.summary.bridgedEndpoints }} bridge
              </UiBadge>
            </div>
          </div>
          <div class="flex shrink-0 flex-wrap items-center gap-2">
            <UiButton
              size="sm"
              variant="outline"
              :disabled="sectionSpeedtestBusy(section)"
              @click="emit('speedtestProviders', providerIdsFromSection(section))"
            >
              <VpIcon name="radar" size-class="size-4" :spin="sectionSpeedtestBusy(section)" />
              Probe
            </UiButton>
            <UiButton
              size="sm"
              variant="outline"
              :disabled="sectionModelRefreshBusy(section)"
              @click="emit('refreshProviderModelsForProviders', providerIdsFromSection(section))"
            >
              <VpIcon
                name="book-open"
                size-class="size-4"
                :spin="sectionModelRefreshBusy(section)"
              />
              Models
            </UiButton>
          </div>
        </div>
      </div>

      <div class="space-y-3 p-4 sm:p-5">
        <ProviderCard
          v-for="card in section.providers"
          :id="`provider-${card.provider.id}`"
          :key="card.provider.id"
          :card="card"
          :health="healthMap[card.provider.id]"
          :creds="credsByProvider[card.provider.id] ?? []"
          :loading-creds="!!loadingCreds[card.provider.id]"
          :toggle-provider-busy="!!toggleBusy[card.provider.id]"
          :circuit-reset-busy="!!circuitResetBusy[card.provider.id]"
          :speedtest-busy="!!speedtestBusy[card.provider.id]"
          :model-refresh-busy="!!modelRefreshBusy[card.provider.id]"
          :cred-model-refresh-busy="credModelRefreshBusy"
          :cred-balance-refresh-busy="credBalanceRefreshBusy"
          :cred-toggle-busy="credToggleBusy"
          :pool-rows="poolRows(card.provider.id)"
          :plan-snap-by-cred="planSnapByCred"
          :active-credential-counts="activeCredentialCountsByProvider[card.provider.id] ?? {}"
          :active-request-count="activeRequestCountsByProvider.get(card.provider.id) ?? 0"
          :tokens-per-sec="tokensPerSec(card.provider.id)"
          :detect-vendor-busy="!!detectVendorBusy[card.provider.id]"
          :class="[
            highlightedProviderId === card.provider.id
              ? 'ring-2 ring-primary/35 ring-offset-2 ring-offset-background'
              : '',
          ]"
          @sync-creds="emit('syncCreds', $event)"
          @detect-vendor="emit('detectVendor', $event)"
          @speedtest-provider="emit('speedtestProvider', $event)"
          @refresh-models="emit('refreshModels', $event)"
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
          @view-logs="emit('viewLogs', $event)"
        />
      </div>
    </UiCard>
  </div>
</template>
