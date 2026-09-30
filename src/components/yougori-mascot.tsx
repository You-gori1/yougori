import "./yougori-mascot.css"

/** A compact silhouette inspired by the elephant reference, with quiet idle motion. */
export function YougoriMascot() {
  return <div className="yougori-mascot" data-tauri-drag-region role="img" aria-label="Yougori elephant mascot">
    <svg className="yougori-mascot-character" viewBox="0 0 112 80" width="58" height="42" aria-hidden="true" focusable="false" fill="currentColor">
      <g className="yougori-mascot-body">
        {/* The open contour between shoulder and head defines the sweeping ear. */}
        <path d="M9 48V29C9 14 22 11 39 9L67 5C52 11 48 19 51 32C54 44 62 48 74 48L73 72Q73 76 69 76H61Q57 76 57 72V53H51V72Q51 76 47 76H43L41 54H32L31 72Q31 76 27 76H20Q16 76 16 72V49L13 56L11 55L12 35Q12 42 9 48Z" />
        <path d="M34 57H39L40 71Q40 76 35 76H32Z" opacity=".7" />
        <g className="yougori-mascot-head">
          <path d="M57 16C68 3 92 4 99 18C102 24 102 31 104 36L96 33C91 32 88 25 83 25C77 25 75 29 77 34L81 40C77 45 68 43 62 39C53 32 53 23 57 16Z" />
          <path d="M83 29C85 34 94 38 106 42C97 41 87 38 81 33Z" />
          <path className="yougori-mascot-trunk" d="M96 35C100 40 103 46 102 53C101 64 94 70 84 70C75 70 70 64 72 57L75 50L82 51L79 58C77 63 81 65 85 64C94 63 96 55 94 49L89 39Z" />
          <path d="M93 23L96 24" fill="none" stroke="var(--background)" strokeWidth="2" strokeLinecap="round" />
        </g>
      </g>
    </svg>
    <span className="yougori-mascot-name" aria-hidden="true">Yougori</span>
  </div>
}
