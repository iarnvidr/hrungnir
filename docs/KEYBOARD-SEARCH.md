# Search and keyboard navigation

Choose a scope beside the search field: All Reddit, the current subreddit, or Downloaded posts. Submitted downloaded searches run locally and show up to 100 matching posts with title/body/comment snippets. Opening a comment match goes to the cached comment and never falls back to a network fetch.

Episode searches such as `gangs of london s02e08` include alternate episode spellings without dropping the show's name. Options lets you turn this off for Reddit searches. Quoted phrases and explicit field/boolean operators remain under your control, for example `title:"episode discussion" subreddit:gangsoflondon`. See Reddit's [documented search features](https://support.reddithelp.com/hc/en-us/articles/19696541895316-Available-search-features). Downloaded searches match episode formats automatically and otherwise treat words literally.

| Context | j / k | l | h |
| --- | --- | --- | --- |
| Post or search list | Focus next / previous visible result | Open focused post | Focus current subreddit in the navigator |
| Subreddit navigator | Focus next / previous community | Fetch that community and enter its post list | Remain in navigator |
| Post heading | Focus a comment (or scroll if there are none) | Enter comments | Return to original post list |
| Comment | Focus next / previous visible comment | Expand replies, then enter the first child | Collapse replies, then focus parent or post heading |
| Cached suggestions | Focus next / previous match | Open cached match | Return to the search field |

Enter opens focused posts/communities or follows focused links. `/` focuses search; Enter submits it and focuses results. Escape leaves the search field while retaining the query and results. Clear explicitly resets search. Escape from the reader returns directly to the list. `?` opens the same shortcut guide available in the header.

Shortcuts do not intercept typing, editable content, media controls, modified keys, or dialogs. Tab/Shift+Tab still reach all controls. Moving focus alone does not fetch posts. Repeated activation keys are ignored to avoid queuing downloads. Comment collapse state and focus survive redraws within the session.
