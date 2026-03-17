import { createCliRenderer } from "@opentui/core";
import { createRoot, useKeyboard, useTerminalDimensions } from "@opentui/react";
import { useCallback, useMemo, useState } from "react";

function App() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [focused, setFocused] = useState<"username" | "password" | "logs">("username");
  const [status, setStatus] = useState("idle");
  const { height } = useTerminalDimensions();
  const form_height = 12;
  const logbox_height = useMemo(() => Math.min(12, height - form_height - 2), [height, form_height]);

  const lines = [
    "This is a simple login form built with OpenTUI.",
    "Use the Tab key to switch between the username and password fields.",
    "Press Enter to submit the form.",
    "The correct credentials are 'admin' for username and 'secret' for password.",
    "The status will be displayed below the form after submission.",
    "Feel free to try different combinations to see the error handling in action!",
    "This example demonstrates how to create interactive CLI applications with OpenTUI.",
    "You can customize the styles and layout as needed to fit your application's requirements.",
    "OpenTUI provides a powerful and flexible way to build rich terminal interfaces with React.",
    "Explore the documentation and examples to learn more about what you can create with OpenTUI!",
    "Happy coding and enjoy building your CLI applications with OpenTUI!",
    "Remember to check out the OpenTUI GitHub repository for updates, bug fixes, and new features.",
    "If you have any questions or need help, don't hesitate to reach out to the OpenTUI community on GitHub or Discord.",
  ];

  useKeyboard((key) => {
    if (key.name === "tab") {
      setFocused((prev) => {
        if (prev === "username") return "password";
        if (prev === "password") return "logs";
        return "username";
      });
    }
  });

  const handleSubmit = useCallback(() => {
    if (username === "admin" && password === "secret") {
      setStatus("success");
    } else {
      setStatus("error");
    }
  }, [username, password]);

  return (
    <box style={{ paddingTop: (height - form_height) / 2 }}>
      <box
        style={{ border: true, padding: 2, flexDirection: "column", gap: 1 }}
      >
        <text fg="#FFFF00">Login Form</text>

        <box title="Username" style={{ border: true, width: 40, height: 3 }}>
          <input
            placeholder="Enter username..."
            onInput={setUsername}
            onSubmit={handleSubmit}
            onMouseDown={() => setFocused("username")}
            focused={focused === "username"}
          />
        </box>

        <box title="Password" style={{ border: true, width: 40, height: 3 }}>
          <input
            placeholder="Enter password..."
            onInput={setPassword}
            onSubmit={handleSubmit}
            onMouseDown={() => setFocused("password")}
            focused={focused === "password"}
          />
        </box>

        <text
          fg={status === "success"
            ? "green"
            : status === "error"
            ? "red"
            : "#999"}
        >
          {status.toUpperCase()}
        </text>

        <text opacity={0.7} >{`Focused: ${focused}`} </text>
      </box>
      <scrollbox style={{ height: logbox_height, border: true }} focusable focused={focused === "logs"}>

        {lines.map((line, index) => <text key={index} selectable={index % 2 === 0}>{line}</text>)}
      </scrollbox>
    </box>
  );
}

const renderer = await createCliRenderer();
createRoot(renderer).render(<App />);
