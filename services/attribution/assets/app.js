"use strict";
document.getElementById("logout")?.addEventListener("click", async (event) => {
  const button = event.currentTarget;
  button.disabled = true;
  try {
    const response = await fetch("/auth/logout", {method: "POST", headers: {"X-CSRF-Token": button.dataset.csrf}});
    if (!response.ok) throw new Error("Sign out failed. Please try again.");
    window.location.assign("/");
  } catch {
    document.getElementById("status").textContent = "Sign out failed. Please try again.";
    button.disabled = false;
  }
});
