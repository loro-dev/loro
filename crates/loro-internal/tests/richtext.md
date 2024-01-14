# Richtext Test Cases

#### 0. Insert into bold span

| Name            | Text                     |
|:----------------|:-------------------------|
| Origin          | `Hello World`            |
| Concurrent A    | `<b>Hello World</b>`     |
| Concurrent B    | `Hello New World`        |
| Expected Result | `<b>Hello New World</b>` |

#### 1. Merge Concurrent Styles

| Name            | Text                 |
|:----------------|:---------------------|
| Origin          | Hello World          |
| Concurrent A    | `<b>Hello</b> World` |
| Concurrent B    | `Hel<b>lo World</b>` |
| Expected Result | `<b>Hello World</b>` |

#### 2. Concurrent insert text & remove style

| Name            | Text                   |
|:----------------|:-----------------------|
| Origin          | `<b>Hello World</b>`   |
| Concurrent A    | `Hello <b>World</b>`   |
| Concurrent B    | `<b>Hello a World</b>` |
| Expected Result | `Hello a <b>World</b>` |

#### 3. Concurrent insert text & style

| Name            | Text                   |
|:----------------|:-----------------------|
| Origin          | `Hello World`          |
| Concurrent A    | `Hello <b>World</b>`   |
| Concurrent B    | `Hello a World`        |
| Expected Result | `Hello a <b>World</b>` |

#### 4. Concurrent text edit & style that shrink

| Name            | Text                       |
|:----------------|:---------------------------|
| Origin          | `Hello World`              |
| Concurrent A    | `<link>Hello</link> World` |
| Concurrent B    | `Hey World`                |
| Expected Result | `<link>Hey</link> World`   |

#### 5. Local insertion expand rules

> [**Hello**](https://www.google.com) World

When insert a new character after "Hello", the new char should be bold but not link

> [**Hello**](https://www.google.com)**t** World


| Name            | Text                              |
|:----------------|:----------------------------------|
| Origin          | `<b><link>Hello</link><b> World`  |
| Expected Result | `<b><link>Hello</link>t<b> World` |


#### 6. Concurrent unbold

In Peritext paper 2.3.2

| Name            | Text                                         |
|:----------------|:---------------------------------------------|
| Origin          | `<b>The fox jumped</b> over the dog.`        |
| Concurrent A    | `The fox jumped over the dog.`               |
| Concurrent B    | `<b>The </b>fox<b> jumped</b> over the dog.` |
| Expected Result | `The fox jumped over the dog.`               |

#### 7. Bold & Unbold

In Peritext paper 2.3.3

| Name            | Text                                         |
|:----------------|:---------------------------------------------|
| Origin          | `<b>The fox jumped</b> over the dog.`        |
| Concurrent A    | `<b>The fox</b> jumped over the dog.`        |
| Concurrent B    | `<b>The</b> fox jumped over the <b>dog</b>.` |
| Expected Result | `<b>The</b> fox jumped over the <b>dog</b>.` |

#### 8. Overlapped formatting

In Peritext paper 3.2, example 3

| Name            | Text                         |
|:----------------|:-----------------------------|
| Origin          | The fox jumped.              |
| Concurrent A    | **The fox** jumped.          |
| Concurrent B    | The *fox jumped*.            |
| Expected Result | **The _fox_**<i> jumped</i>. |

#### 9. Multiple instances of the same mark

![](https://i.postimg.cc/MTNGq8cH/Clean-Shot-2023-10-09-at-12-16-29-2x.png)
