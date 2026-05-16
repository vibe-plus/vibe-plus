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
  <div class="space-y-3">
    <div v-for="section in sections" :key="section.key" class="space-y-2.5">
      <div class="rounded-lg border border-slate-200 bg-white px-3 py-2 shadow-sm">
        <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
          <div class="min-w-0 flex-1">
            <div class="flex min-w-0 flex-wrap items-center gap-2">
              <span :class="['i-lucide-layers-3', 'size-4 text-slate-500']" aria-hidden="true" />
              <h2 class="text-sm font-semibold text-slate-900">{{ section.title }}</h2>
              <span
                class="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5 text-[11px] text-slate-600"
              >
                {{ section.summary.enabledEndpoints }}/{{ section.summary.totalEndpoints }}
                endpoints
              </span>
              <span
                v-if="section.summary.activeRequests"
                class="rounded-full border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-[11px] text-emerald-700"
              >
                live {{ section.summary.activeRequests }}
              </span>
              <span
                v-if="section.summary.blockedCredentials"
                class="rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 text-[11px] text-amber-800"
              >
                {{ section.summary.blockedCredentials }} blocked creds
              </span>
            </div>
            <div
              class="mt-2 grid grid-cols-2 gap-1.5 text-[11px] text-slate-500 sm:grid-cols-3 lg:grid-cols-4"
            >
              <span class="rounded-md bg-slate-50 px-2 py-1"
                >{{ section.summary.availableCredentials }}/{{
                  section.summary.enabledCredentials
                }}
                credentials</span
              >
              <span class="rounded-md bg-slate-50 px-2 py-1">{{
                section.summary.fastestLatencyMs == null
                  ? "no speed"
                  : `${Math.round(section.summary.fastestLatencyMs)}ms best`
              }}</span>
              <span class="rounded-md bg-slate-50 px-2 py-1"
                >{{ section.summary.remoteModels }} models</span
              >
              <span class="rounded-md bg-slate-50 px-2 py-1"
                >{{ section.summary.nativeEndpoints }} native /
                {{ section.summary.bridgedEndpoints }} bridge</span
              >
            </div>
          </div>
          <div class="flex shrink-0 items-center gap-2">
            <button
              type="button"
              class="btn-ghost inline-flex items-center gap-1.5 px-2.5 py-1.5 text-xs"
              :disabled="sectionSpeedtestBusy(section)"
              @click="emit('speedtestProviders', providerIdsFromSection(section))"
            >
              <VpIcon name="radar" size-class="size-3.5" :spin="sectionSpeedtestBusy(section)" />
              probe
            </button>
            <button
              type="button"
              class="btn-ghost inline-flex items-center gap-1.5 px-2.5 py-1.5 text-xs"
              :disabled="sectionModelRefreshBusy(section)"
              @click="emit('refreshProviderModelsForProviders', providerIdsFromSection(section))"
            >
              <VpIcon
                name="book-open"
                size-class="size-3.5"
                :spin="sectionModelRefreshBusy(section)"
              />
              models
            </button>
          </div>
        </div>
      </div>

      <div class="grid grid-cols-1 gap-3">
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
              ? 'ring-2 ring-sky-300 ring-offset-2 ring-offset-vp-bg'
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
    </div>
  </div>
</template>
