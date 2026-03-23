module.exports = {
	content: ["./templates/**/*.html", "./src/**/*.rs"],
	theme: {
		extend: {
			boxShadow: {
				soft: "0 1px 2px rgba(26, 15, 42, 0.04), 0 8px 24px rgba(26, 15, 42, 0.08)",
			},
		},
	},
	plugins: [],
};
