import { LoroDoc } from "../bundler/index";
import { beforeEach, describe, expect, it } from "vitest";

describe("JSONPath", () => {
    let doc: LoroDoc;

    beforeEach(() => {
        doc = new LoroDoc();
        const testData = {
            books: [
                { title: "1984", author: "George Orwell", price: 10, available: true },
                { title: "Animal Farm", author: "George Orwell", price: 8, available: true },
                { title: "Brave New World", author: "Aldous Huxley", price: 12, available: false },
                { title: "Fahrenheit 451", author: "Ray Bradbury", price: 9, available: true },
                { title: "The Great Gatsby", author: "F. Scott Fitzgerald", price: null, available: true },
                { title: "To Kill a Mockingbird", author: "Harper Lee", price: 11, available: true },
                { title: "The Catcher in the Rye", author: "J.D. Salinger", price: 10, available: false },
                { title: "Lord of the Flies", author: "William Golding", price: 9, available: true },
                { title: "Pride and Prejudice", author: "Jane Austen", price: 7, available: true },
                { title: "The Hobbit", author: "J.R.R. Tolkien", price: 14, available: true }
            ],
            featured_author: "George Orwell",
            min_price: 10,
            featured_authors: ["George Orwell", "Jane Austen"]
        };
        const store = doc.getMap("store");
        Object.entries(testData).forEach(([key, value]) => {
            store.set(key, value);
        })

        const project = doc.getMap("project");
        project.set("name", "Launch Plan");
        project.set("tasks", [
            { id: 1, title: "Storyboard slides", assignee: "amy", status: "in-progress" },
            { id: 2, title: "Budget review", assignee: "li", status: "todo" },
            { id: 3, title: "Finalize keynote deck", assignee: "amy", status: "done" }
        ]);

        const drafts = doc.getList("drafts");
        drafts.push({ title: "slide walkthrough" });
        drafts.push({ title: "executive summary" });
        drafts.push({ title: "slide qa checklist" });

        const todos = doc.getList("todos");
        todos.push({ title: "Wire up auth", status: "done" });
        todos.push({ title: "Polish animation", status: "doing" });
        todos.push({ title: "Ship launch blog", status: "done" });

        doc.commit();
    });

    describe("basic jsonpath parsing", () => {
        it("parses basic path correctly", () => {
            const path = "$['store'].books[0].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("1984");
        });
    });

    describe("jsonpath selectors", () => {
        it("handles child selectors", () => {
            const path = "$['store'].books[0].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("1984");
        });

        it("handles wildcard selector", () => {
            const path = "$['store'].books[*].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(10); // 10 books
            expect(result).toEqual([
                "1984",
                "Animal Farm",
                "Brave New World",
                "Fahrenheit 451",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "The Catcher in the Rye",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit"
            ]);
        });

        it("handles recursive descent", () => {
            const path = "$..title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(19);
        });

        it("handles quoted keys", () => {
            const path = "$['store']['books'][0]['title']";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("1984");
        });
    });

    describe("string filters", () => {
        it("filters by exact string match", () => {
            const path = "$['store'].books[?(@.title == '1984')].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("1984");
        });

        it("filters by string contains", () => {
            const path = "$['store'].books[?(@.title contains 'Farm')].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("Animal Farm");
        });

        it("filters by recursive string match", () => {
            const path = "$..[?(@.author contains 'Orwell')].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2); // 2 Orwell books
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
        });
    });

    describe("logical operators", () => {
        it("filters with AND operator", () => {
            const path = `$['store'].books[?(@.author == "George Orwell" && @.price < 10)].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("Animal Farm");
        });

        it("filters with OR operator", () => {
            const path = `$['store'].books[?(@.author == "George Orwell" || @.price >= 10)].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(6);
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
            expect(result).toContain("Brave New World");
            expect(result).toContain("To Kill a Mockingbird");
            expect(result).toContain("The Catcher in the Rye");
            expect(result).toContain("The Hobbit");
        });

        it("filters with complex AND/OR combination", () => {
            const path = `$['store'].books[?(@.author == "George Orwell" && (@.price < 10 || @.available == true))].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
        });

        it("filters with NOT operator", () => {
            const path = "$['store'].books[?(!(@.available == false))].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(8); // 10 books - 2 unavailable (Brave New World, The Catcher in the Rye)
            expect(result).toEqual([
                "1984",
                "Animal Farm",
                "Fahrenheit 451",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit"
            ]);
        });
    });

    describe("in operator", () => {
        it("filters by author in list", () => {
            const path = `$['store'].books[?(@.author in ["George Orwell", "Jane Austen"])].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
            expect(result).toContain("Pride and Prejudice");
        });

        it("filters by price in list", () => {
            const path = `$['store'].books[?(@.price in [7, 10, 14])].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(4);
            expect(result).toContain("1984");
            expect(result).toContain("Pride and Prejudice");
            expect(result).toContain("The Catcher in the Rye");
            expect(result).toContain("The Hobbit");
        });

        it("filters with in operator and null values", () => {
            const path = `$['store'].books[?(@.price in [null, 9])].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            expect(result).toContain("Fahrenheit 451");
            expect(result).toContain("Lord of the Flies");
            expect(result).toContain("The Great Gatsby");
        });

        it("filters with in operator in recursive descent", () => {
            const path = `$..[?(@.author in ["George Orwell", "Ray Bradbury"])].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
            expect(result).toContain("Fahrenheit 451");
        });

        it("filters with root list in", () => {
            const path = "$.store.books[?(@.author in $.store.featured_authors)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            result.sort();
            const expected = ["1984", "Animal Farm", "Pride and Prejudice"];
            expected.sort();
            expect(result).toEqual(expected);
        });
    });

    describe("union and slice operations", () => {
        it("handles union indexes", () => {
            const path = "$['store'].books[0,2].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect(result[0]).toBe("1984");
            expect(result[1]).toBe("Brave New World");
        });

        it("handles union keys", () => {
            const path = "$['store'].books[0]['title','author']";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect(result[0]).toBe("1984");
            expect(result[1]).toBe("George Orwell");
        });

        it("handles union with negative indexes", () => {
            const path = "$['store'].books[-2,-1].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect(result[0]).toBe("Pride and Prejudice");
            expect(result[1]).toBe("The Hobbit");
        });

        it("handles basic slice", () => {
            const path = "$['store'].books[0:3].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            expect(result).toEqual(["1984", "Animal Farm", "Brave New World"]);
        });

        it("handles slice with step", () => {
            const path = "$['store'].books[0:5:2].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            expect(result).toEqual(["1984", "Brave New World", "The Great Gatsby"]);
        });

        it("handles negative slice", () => {
            const path = "$['store'].books[-2:].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect(result[0]).toBe("Pride and Prejudice");
            expect(result[1]).toBe("The Hobbit");
        });
    });

    describe("complex and recursive filters", () => {
        it("filters with multiple conditions", () => {
            const path = "$['store'].books[?(@.price >= 10 && @.available == true && @.title contains '1984')].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("1984");
        });

        it("filters with path expressions", () => {
            const path = `$['store'].books[?(@.author == "George Orwell" && @.title != "1984")].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("Animal Farm");
        });

        it("filters with null checks", () => {
            const path = "$['store'].books[?(@.price == null || @.price < 10)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(5);
            expect(result).toContain("Animal Farm");
            expect(result).toContain("Fahrenheit 451");
            expect(result).toContain("The Great Gatsby");
            expect(result).toContain("Pride and Prejudice");
            expect(result).toContain("Lord of the Flies");
        });

        it("handles recursive filter with price condition", () => {
            const path = "$..[?(@.price > 10)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            expect(result).toContain("Brave New World");
            expect(result).toContain("The Hobbit");
            expect(result).toContain("To Kill a Mockingbird");
        });

        it("handles recursive filter with logical operators", () => {
            const path = `$..[?(@.author == "George Orwell" || @.price > 10)].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(5);
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
            expect(result).toContain("Brave New World");
            expect(result).toContain("To Kill a Mockingbird");
            expect(result).toContain("The Hobbit");
        });
    });

    describe("edge cases and error handling", () => {
        it.todo("handles quoted keys with special characters", () => {
            const specialDoc = new LoroDoc();
            specialDoc.getMap("root").set("book", {
                map: { "book-with-dash": { "price-$10": "cheap" } }
            });
            specialDoc.commit();
            const path = "$.root['map']['book-with-dash']['price-$10']";
            const result = specialDoc.JSONPath(path);
            expect(result).toHaveLength(1);
            expect(result[0]).toBe("cheap");
        });

        it("handles quoted keys with escaped quotes", () => {
            const specialDoc = new LoroDoc();
            specialDoc.getMap("root").set("book", { map: { 'book-with-"quote"': { 'price-"10"': "moderate" } } });
            specialDoc.commit();
            const path = `$['store'].books[?(@.author == "George Orwell")].title`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect(result).toContain("1984");
            expect(result).toContain("Animal Farm");
        });
    });

    describe("filters with root references", () => {
        it("filters with root reference", () => {
            const path = "$.store.books[?(@.author == $.store.featured_author)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            result.sort();
            const expected = ["1984", "Animal Farm"];
            expected.sort();
            expect(result).toEqual(expected);
        });

        it("filters with root numerical comparison", () => {
            const path = "$.store.books[?(@.price > $.store.min_price)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(3);
            result.sort();
            const expected = ["Brave New World", "The Hobbit", "To Kill a Mockingbird"];
            expected.sort();
            expect(result).toEqual(expected);
        });

        it("filters with root not equal", () => {
            const path = "$.store.books[?(@.author != $.store.featured_author)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(8);
            result.sort();
            const expected = [
                "Brave New World",
                "Fahrenheit 451",
                "The Great Gatsby",
                "To Kill a Mockingbird",
                "The Catcher in the Rye",
                "Lord of the Flies",
                "Pride and Prejudice",
                "The Hobbit"
            ];
            expected.sort();
            expect(result).toEqual(expected);
        });

        it("filters with root complex", () => {
            const path = "$.store.books[?(@.author == $.store.featured_author && @.price <= $.store.min_price)].title";
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            result.sort();
            const expected = ["1984", "Animal Farm"];
            expected.sort();
            expect(result).toEqual(expected);
        });
    });

    describe("discord example queries", () => {
        it("finds every task assigned to amy", () => {
            const path = `$.project.tasks[?(@.assignee in ["amy"])]`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            const titles = result.map((task: any) => task.title).sort();
            const expected = ["Storyboard slides", "Finalize keynote deck"].sort();
            expect(titles).toEqual(expected);
        });

        it("selects drafts that mention slide", () => {
            const path = `$.drafts[?(@.title contains "slide")]`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            const titles = result.map((draft: any) => draft.title);
            expect(titles).toContain("slide walkthrough");
            expect(titles).toContain("slide qa checklist");
        });

        it("grabs the first completed todo", () => {
            const path = `$.todos[?(@.status == "done")]`;
            const result = doc.JSONPath(path);
            expect(result).toHaveLength(2);
            expect((result[0] as any).title).toBe("Wire up auth");
        });
    });
});
