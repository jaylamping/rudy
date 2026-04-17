Revisit consent button

We value your privacy

We use cookies to enhance your browsing experience, serve personalized ads or content, and analyze our traffic. By clicking "Accept All", you consent to our use of cookies.

CustomizeAccept All

Customize Consent PreferencesClose

We use cookies to help you navigate efficiently and perform certain functions. You will find detailed information about all cookies under each consent category below.

The cookies that are categorized as "Necessary" are stored on your browser as they are essential for enabling the basic functionalities of the site. ... Show more

NecessaryAlways Active

Necessary cookies are required to enable the basic features of this site, such as providing secure log-in or adjusting your consent preferences. These cookies do not store any personally identifiable data.

- Cookie

wpEmojiSettingsSupports

- Duration

session

- Description

WordPress sets this cookie when a user interacts with emojis on a WordPress site. It helps determine if the user's browser can display emojis properly.

- Cookie

cookieyes-consent

- Duration

1 year

- Description

CookieYes sets this cookie to remember users' consent preferences so that their preferences are respected on subsequent visits to this site. It does not collect or store any personal information about the site visitors.

- Cookie

cfuvid

- Duration

session

- Description

Calendly sets this cookie to track users across sessions to optimize user experience by maintaining session consistency and providing personalized services

- Cookie

cfruid

- Duration

session

- Description

Cloudflare sets this cookie to identify trusted web traffic.

- Cookie

m

- Duration

1 year 1 month 4 days

- Description

Stripe sets this cookie for fraud prevention purposes. It identifies the device used to access the website, allowing the website to be formatted accordingly.

Functional

Functional cookies help perform certain functionalities like sharing the content of the website on social media platforms, collecting feedback, and other third-party features.

- Cookie

referreruserid

- Duration

14 days

- Description

Calendly sets this cookie for the booking functionality of the website.

Analytics

Analytical cookies are used to understand how visitors interact with the website. These cookies help provide information on metrics such as the number of visitors, bounce rate, traffic source, etc.

- Cookie

ga

- Duration

1 year 1 month 4 days

- Description

Google Analytics sets this cookie to store and count page views.

- Cookie

ga

- Duration

1 year 1 month 4 days

- Description

Google Analytics sets this cookie to calculate visitor, session and campaign data and track site usage for the site's analytics report. The cookie stores information anonymously and assigns a randomly generated number to recognise unique visitors.

- Cookie

ajsuserid

- Duration

Never Expires

- Description

This cookie is set by Segment to help track visitor usage, events, target marketing, and also measure application performance and stability.

- Cookie

ajsgroupid

- Duration

Never Expires

- Description

This cookie is set by Segment to track visitor usage and events within the website.

- Cookie

pendovisitorId.4cfbcefc-fcf9-4b66-5dc6-9b0d81bb07a9

- Duration

Never Expires

- Description

Pendo sets this cookie to identify and record the visitor’s Account ID that will be used in Pendo, like Guide delivery and analytics.

- Cookie

ajsanonymousid

- Duration

Never Expires

- Description

This cookie is set by Segment to count the number of people who visit a certain site by tracking if they have visited before.

- Cookie

pendoguidesblocked.4cfbcefc-fcf9-4b66-5dc6-9b0d81bb07a9

- Duration

Never Expires

- Description

Pendo sets this cookie to identify and record the visitor’s Account ID that will be used in Pendo, like Guide delivery and analytics.

Performance

Performance cookies are used to understand and analyze the key performance indexes of the website which helps in delivering a better user experience for the visitors.

- Cookie

calendlysession

- Duration

21 days

- Description

Calendly, a Meeting Schedulers, sets this cookie to allow the meeting scheduler to function within the website and to add events into the visitor’s calendar.

Advertisement

Advertisement cookies are used to provide visitors with customized advertisements based on the pages you visited previously and to analyze the effectiveness of the ad campaigns.

No cookies to display.

Uncategorised

Other uncategorised cookies are those that are being analysed and have not been classified into a category as yet.

- Cookie

cfclearance

- Duration

1 year

- Description

Description is currently not available.

Save My Preferences  Accept All

Powered by [Cookieyes logo](https://www.cookieyes.com/product/cookie-consent)

[Skip to content](https://henkirobotics.com/ros-2-best-practices/#content)

Henki ROS 2 Best Practices repository logo

# ROS 2 Best Practices

By [Janne Karttunen](https://henkirobotics.com/author/admin/)  February 16, 2026

Good architecture and clean coding practices are fundamental for robotics projects to scale and evolve quickly in response to rapidly changing requirements. A messy codebase and short-term shortcuts may deliver fast results at the beginning, but as the project progresses, implementing new changes becomes increasingly time-consuming and carries a higher risk of breaking existing functionality.

ROS 2 best practices are architectural and development guidelines that improve scalability, maintainability, testing, performance, and long-term sustainability of robotics software built with ROS 2.

Currently, information about best practices in ROS 2 is scattered. As a result, many companies often end up compiling their own internal guidelines.

ROS projects usually face similar questions:

- What are the best ROS 2 practices?
- How can these best practices be enforced in projects?
- How can AI coding agents be guided to comply with these best practices?

## Announcing the Henki ROS 2 Best Practices

To address the questions above, we decided to write down our own ROS 2 best practices at Henki Robotics – the guidelines we have been gathering and following over the many years we have been working with ROS 2. As an innovative step, these practices can be also very easily integrated with AI coding agents, enabling consistent code quality.

Check out the full list of the best practices in our GitHub repository: [Henki ROS 2 Best Practices](https://github.com/henki-robotics/henki_ros2_best_practices).

## ROS 2 Best Practices

Here is a summary of our top picks from the best practices described in the repository, which can quickly improve your project architecture and design.

- **Nodes:** Follow the single-responsibility principle when writing ROS 2 nodes, and separate ROS 2 communication logic from core application logic.
- **Launch files:** There has been recent discussion about the preferred launch format. Prefer XML, as the current Python launch files were not intended to be used as a launching system front-end.
- **Parameters and Configuration files:** Define node parameters in parameter files instead of hard-coding them in nodes or launch files.
- **Logging:** Start using clean logging practices from the very beginning. Use the correct log levels and avoid log spam.
- **Message Interfaces**: Use existing ROS 2 message interfaces when they fit your data. Otherwise, create custom message interfaces with a low threshold. Do not use the deprecated primitive type interfaces such as `Bool`, `String` and `Float32` from `std_msgs`.
- **Actions and Services:** Use services only for fast-executing tasks (less than a second) and actions for tasks that take time, may fail for different reasons, or should be cancellable.
- **Code style:** ROS 2 already defines excellent [code style guidelines](https://docs.ros.org/en/rolling/The-ROS2-Project/Contributing/Code-Style-Language-Versions.html). Summary: Google style for C++ (with modifications) and PEP8 for Python.
- **Performance:** Use C++ for high-bandwidth processing instead of Python. Use composable nodes to avoid passing large data over DDS.
- **Executors and Callback Groups:** Use `MultiThreadedExecutors` only when absolutely necessary, as they can add performance overhead. Single-threaded applications are often cleaner and easier to test.
- **Dependencies:** Use the default ROS style with `rosdep` to handle all package dependencies. `rosdep` isn’t perfect: it is not possible to choose the dependency versions, and not all libraries are available through it. Follow our tips for using `requirements.txt` and `.repos` files.
- **Documentation:** At a minimum, provide package-level documentation describing the purpose of the package and all exposed topics, services, actions and parameters.
- **Testing:** Aim for high test coverage. Test core application logic with unit tests at a minimum. Test ROS 2 communication logic as part of integration tests, not unit tests.

More best practices and detailed explanations for each point are available in the main [Henki ROS 2 Best Practices](https://github.com/henki-robotics/henki_ros2_best_practices) repository.

## Enforcing the Best Practices

Tests and code style are usually straightforward to enforce in a project by integrating automatic code coverage, linting, and formatting tools into the Continuous Integration (CI) pipelines.

However, enforcing best practices is less direct – it’s largely a matter of design, architecture, and writing guidelines. In practice, best practices are often enforced during code reviews. Knowing the best practices is one thing; ensuring that a project actually follows them is another.

For example, if a new developer joins an open-source ROS 2 project, they might hit roadblocks during the review process when a maintainer requests changes to ensure message interfaces are used correctly or that the executor choice is appropriate. Without clearly documented guidelines, new developers have no reference for writing good ROS 2 code. This responsibility typically falls on more experienced developers.

But how can an independent developer ensure best practices in their own project? Or how can a robotics startup write efficient, maintainable code from the very beginning?

This reliance on experienced developers to enforce best practices is beginning to shift as AI gradually starts to take over code writing.

## Integration Of The Best Practices With AI Agents

AI tools are becoming increasingly capable of writing code, which raises an important question: what is the role of software developers in the AI era?

It will shift more toward ideas, design, and architecture. Code quality remains critical – we still need to understand and review the code AI produces. Developers provide the instructions for how the code should be structured and what requirements it must meet.

This is exactly the goal of **Henki ROS 2 Best Practices**. It’s not only a set of guidelines for humans, but it can also be directly integrated with AI agents, enabling them to write code according to these best practices while also helping us review that existing code follows these practices.

## Example Package Generation Using Best Practices

In our best practices repository, we included an example demonstrating how AI-generated code quality improves when these best practices are applied. While the example is intentionally simple, the same principles scale to larger packages and projects.

We used Claude Code (claude-opus-4-6) to complete the following task, both before and after applying our best practices:

*Create me a new ROS 2 package with a Python node that subscribes to /odom topic and publishes to a single topic the robot speed in km/h and mph.*

### Before Applying the Best Practices

By default, before integrating any of our best practices in Claude’s workflow, the generated node and ROS 2 project structure are fairly standard.

Project structure before applying ROS 2 best practices*Project structure generated by Claude without ROS 2 best practices*

The project includes a basic `speed_monitor_node.py`, with the necessary ROS 2 package files, such as `package.xml` and `setup.py`.

Example ROS 2 node code before applying ROS 2 best practices*ROS 2 node generated by Claude without ROS 2 best practices*

The generated Node is also pretty straightforward: It creates a subscription to the `/odom` topic, and publishes the robot speed to the `/speed` topic using `Float64MultiArray` as the message type. The core application logic, the speed calculation, happens directly inside the odometry callback.

### After Applying the Best Practices

After cloning the **henkiros2bestpractices** repository, we added the following text to the `CLAUDE.md` file to provide instructions for our coding agent:

```
Always read and follow the best practices defined in:
- henki_ros2_best_practices/henki_ros2_best_practices.md
```

When asking Claude to generate the package again with the exact same prompt, the improvements in the generated code were immediately noticeable.

Project structure after applying ROS 2 best practices*Claude generated project structure with ROS 2 best practices applied*

The package structure is now significantly different from what we saw previously. Instead of a single `speed_monitor` package, there are two packages: `speed_reporter` and `speed_reporter_msgs`. Claude generated a new custom message interface, `RobotSpeed.msg`, which describes the robot speed data much more clearly than the deprecated `Float64MultiArray` format.

In addition, the `speed_reporter` package now includes:

- Configuration and launch files.
- Documentation of the Node API in the `README.md` file.
- Core application logic separated into `speed_converter.py`, decoupled from the ROS 2 node communication layer.
- Unit tests that target only the core application logic.

Example ROS 2 node code after applying ROS 2 best practices*Claude generated node with ROS 2 best practices applied*Example application code after applying ROS 2 best practices*When best practices were applied, Claude split the core application logic into its own class*

Looking at the code, the differences compared to the previously generated version are clear:

- Topics and QoS settings are defined in parameter files, allowing end users to modify them through configuration changes rather than code edits or remaps in the launch file.
- The core application logic – the speed conversion – has been extracted into its own class. This makes it straightforward to test the logic in unit tests without involving the ROS 2 communication layer. The benefits become even more apparent as complexity grows. Imagine, for example, having core autonomous planning or SLAM logic fully separated from ROS. This whole topic would deserve a blog post of its own.
- Logger was previously printing an info message at the end of every odometry callback. If odometry is published at 30hz, that results in 30 log messages per second. With the best practices applied, Claude now used throttled log, to print the info message only once per second to avoid log spam.

The benefits of applying these best practices are already visible in a small example like this. As project size and complexity increase, their importance only grows.

All the example code is available in our [Agentic Examples](https://github.com/henki-robotics/henki_ros2_agentic_examples/) repository.

## Summary

Scalable and maintainable ROS 2 systems require clear architecture and consistent best practices. **Henki ROS 2 Best Practices** provide a structured foundation for both developers and AI coding agents, improving code quality and enforceability.

As software developers, our role is shifting from manually writing code to defining **how** code should be written, ensuring it follows established best practices.

- **GitHub Repository:** [Henki ROS 2 Best Practices](https://github.com/henki-robotics/henki_ros2_best_practices)

### Related posts:

1. [Student Story From Robotics Course: When Your Robot’s Enemy is a Plank](https://henkirobotics.com/student-story-from-robotics-course/)
2. [Why do robotics companies choose not to contribute to open source?](https://henkirobotics.com/why-do-robotics-companies-choose-not-to-contribute-to-open-source/)
3. [Create ROS 2 Packages with Turtle Nest](https://henkirobotics.com/create-ros-2-packages-with-turtle-nest/)
4. [Robotics and ROS 2 Essentials – Course Announcement](https://henkirobotics.com/robotics-and-ros-2-essentials-course-announcement/)