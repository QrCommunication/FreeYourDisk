import { Broom, Files, GitBranch, Stack } from "phosphor-svelte";
import type { Component } from "svelte";
import type { ServiceId } from "./api";

export const serviceIcon: Record<ServiceId, Component> = {
  temp: Broom,
  big_files: Files,
  git_repos: GitBranch,
  dev_cache: Stack,
};
