plugins {
  kotlin("jvm") version "1.9.22"
}

dependencies {
  implementation("com.fasterxml.jackson.core:jackson-databind:2.16.1")
  implementation(platform("org.junit:junit-bom:5.10.2"))
  api "org.slf4j:slf4j-api:2.0.12"
  compileOnly("org.projectlombok:lombok:1.18.30")
  testImplementation("org.junit.jupiter:junit-jupiter:5.10.2")
  implementation(group: "com.acme", name: "sdk", version: "2.0.0")
  implementation(project(":common"))
}
