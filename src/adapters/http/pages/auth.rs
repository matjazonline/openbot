//! Login and registration pages.

use super::*;

pub fn login_page() -> String {
    let content = r##"
        <h2 class="text-2xl font-bold text-white mb-6 text-center">Welcome back</h2>

        <div id="response-message" class="mb-4"></div>

        <form hx-post="/api/user/login" hx-target="#response-message" hx-swap="innerHTML" class="space-y-5 max-w-md mx-auto">
            <div>
                <label for="email_or_username" class="block text-sm font-medium text-slate-300 mb-1">Email or Username</label>
                <input type="text" id="email_or_username" name="email_or_username" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="you@example.com or username">
            </div>

            <div>
                <label for="password" class="block text-sm font-medium text-slate-300 mb-1">Password</label>
                <input type="password" id="password" name="password" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="........">
            </div>

            <button type="submit"
                class="w-full py-3 px-4 bg-indigo-600 hover:bg-indigo-500 text-white font-semibold rounded-xl shadow-lg shadow-indigo-600/30 transition duration-150 ease-in-out cursor-pointer flex items-center justify-center">
                <span>Sign In</span>
            </button>
        </form>

        <div class="mt-6 text-center text-sm text-slate-400">
            Don't have an account?
            <a href="/register" class="text-indigo-400 hover:text-indigo-300 font-medium ml-1 transition">Sign up</a>
        </div>
    "##;

    public_layout("Login", content)
}

pub fn register_page() -> String {
    let content = r##"
        <h2 class="text-2xl font-bold text-white mb-6 text-center">Create an account</h2>

        <div id="response-message" class="mb-4"></div>

        <form hx-post="/api/user/register" hx-target="#response-message" hx-swap="innerHTML" class="space-y-4 max-w-md mx-auto">
            <div>
                <label for="username" class="block text-sm font-medium text-slate-300 mb-1">Username</label>
                <input type="text" id="username" name="username" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="johndoe">
            </div>

            <div>
                <label for="email" class="block text-sm font-medium text-slate-300 mb-1">Email address</label>
                <input type="email" id="email" name="email" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="you@example.com">
            </div>

            <div>
                <label for="password" class="block text-sm font-medium text-slate-300 mb-1">Password</label>
                <input type="password" id="password" name="password" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="........">
            </div>

            <div>
                <label for="confirm_password" class="block text-sm font-medium text-slate-300 mb-1">Confirm Password</label>
                <input type="password" id="confirm_password" name="confirm_password" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="........">
            </div>

            <button type="submit"
                class="w-full py-3 px-4 bg-indigo-600 hover:bg-indigo-500 text-white font-semibold rounded-xl shadow-lg shadow-indigo-600/30 transition duration-150 ease-in-out cursor-pointer flex items-center justify-center mt-2">
                <span>Create Account</span>
            </button>
        </form>

        <div class="mt-6 text-center text-sm text-slate-400">
            Already have an account?
            <a href="/login" class="text-indigo-400 hover:text-indigo-300 font-medium ml-1 transition">Sign in</a>
        </div>
    "##;

    public_layout("Register", content)
}
